#![warn(clippy::cargo)]

use clap::{ArgAction, Parser};
use colored::Colorize;
use die_exit::{Die, DieWith};
use glob::{Paths, glob};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::{borrow::Cow, io};
use tokio::fs;
use urlencoding::decode;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Parser, Default)]
#[command(author, version, about, long_about = None, after_help = r#"Examples:
urldecoder test/t.md        # decode test/t.md
urldecoder *.md -e my.md    # decode all markdown files in current folder except `my.md`
urldecoder **/*             # decode all files recursively in current folder
"#)]
pub struct Cli {
    /// Files to convert, uses glob("{file}") to parse given pattern
    #[clap(required = true)]
    files: Vec<PathBuf>,
    /// Show result only, without overwrite
    #[arg(short, long)]
    dry_run: bool,
    /// Show full debug and error message
    #[arg(short, long)]
    verbose: bool,
    /// Exclude file or folder
    #[arg(short, long, action = ArgAction::Append)]
    exclude: Vec<PathBuf>,
    /// Do not decode `%20` to space
    #[arg(long)]
    escape_space: bool,
}

#[derive(Debug, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
enum EndOfLine {
    LF,
    CRLF,
}
impl EndOfLine {
    pub fn as_str(&self) -> &'static str {
        match self {
            EndOfLine::LF => "\n",
            EndOfLine::CRLF => "\r\n",
        }
    }
}

/// Whether a file in exclude list.
fn in_exclude<'a, T>(exclude: T, pattern: &'a Path) -> bool
where
    T: IntoIterator<Item = &'a PathBuf>,
{
    exclude.into_iter().any(|p| pattern.strip_prefix(p).is_ok())
}

/// Detect if the file uses LF or CRLF. Returns the line ending, `\r\n` for CRLF
/// and `\n` for LF.
fn detect_lf_crlf(s: &str) -> EndOfLine {
    let bytes = s.as_bytes();
    let pos = bytes.iter().rposition(|&b| b == b'\n');
    match pos {
        Some(p) => {
            if p > 0 && bytes[p - 1] == b'\r' {
                EndOfLine::CRLF
            } else {
                EndOfLine::LF
            }
        }
        None => EndOfLine::LF,
    }
}

/// Find all urls in the code and decode them.
/// Returns the String of decoded code and a bool indicates whether the code has
/// decoded urls.
fn decode_url_in_code(code: &str, escape_space: bool) -> (String, bool) {
    let mut replaced = false;
    let regex =
        Regex::new(r#"https?://[-A-Za-z0-9+&@#/%?=~_|!:,.;]+[-A-Za-z0-9+&@#/%=~_|]"#).unwrap();
    (
        regex
            .replace_all(code, |caps: &regex::Captures| {
                let url = &caps[0];
                if url.rfind('%').is_none() {
                    return url.to_owned();
                }
                let mut decoded_url = decode(url).unwrap_or(Cow::Borrowed(url));
                let result = if escape_space {
                    // Replacing after decoding will not affect much performance (Benchmarked).
                    decoded_url.to_mut().replace(' ', "%20")
                } else {
                    decoded_url.into()
                };
                if url == result {
                    return url.to_owned();
                }
                replaced = true;
                result
            })
            .into_owned(),
        replaced,
    )
}

async fn process_file(
    file_path: &PathBuf,
    verbose: bool,
    escape_space: bool,
    dry_run: bool,
) -> io::Result<()> {
    let mut replaced = false;
    let content = fs::read_to_string(&file_path).await?;
    let lf_crlf = detect_lf_crlf(&content);
    if verbose {
        println!(
            "Processing {}, End of line: {:?}",
            file_path.display(),
            lf_crlf
        );
    }
    let mut decoded_content = String::new();
    for (line_number, line) in content.lines().enumerate() {
        let (decoded_line, replaced_line) = decode_url_in_code(line, escape_space);
        if replaced_line {
            if !replaced {
                println!("In file: {}", file_path.display());
                replaced = true;
            }
            println!(
                "{}\n{}",
                format!("{} - {}", line_number + 1, line).red(),
                format!("{} + {}", line_number + 1, decoded_line).green()
            )
        }
        decoded_content.push_str(&decoded_line);
        decoded_content.push_str(lf_crlf.as_str());
    }
    if replaced && !dry_run {
        for _ in 0..lf_crlf.as_str().len() {
            decoded_content.pop(); // remove the last '\n' or '\r\n'.
        }
        fs::write(&file_path, decoded_content).await?;
    }
    Ok(())
}

async fn process_directory(
    files: Vec<PathBuf>,
    exclude: Vec<PathBuf>,
    verbose: bool,
    escape_space: bool,
    dry_run: bool,
) -> Result<()> {
    let pathss: Vec<Paths> = files
        .iter()
        .map(|p| {
            glob(
                p.to_str()
                    .die(format!("Parsing invalid filename: {}", p.display()).as_str()),
            )
            .die_with(|e| e.to_string())
        })
        .collect();
    let mut handles = Vec::new();
    for entry in pathss.into_iter().flatten() {
        let entry = entry?;
        if !entry.is_file() || in_exclude(&exclude, &entry) {
            continue;
        }
        let handle = tokio::spawn(async move {
            if let Err(err) = process_file(&entry, verbose, escape_space, dry_run).await {
                if verbose || err.kind() != io::ErrorKind::InvalidData {
                    eprintln!("ERROR: {} : {}", err, entry.display())
                };
            }
        });
        handles.push(handle);
    }
    for handle in handles {
        handle.await?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();
    cli.exclude.push("node_modules".into());
    cli.exclude.dedup();
    process_directory(
        cli.files,
        cli.exclude,
        cli.verbose,
        cli.escape_space,
        cli.dry_run,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use temp_testdir::TempDir;

    #[test]
    fn test_detect_lf_crlf() {
        assert!(detect_lf_crlf("asd\r\n\rda") == EndOfLine::CRLF);
        assert!(detect_lf_crlf("asd\n\rda\n") == EndOfLine::LF);
    }

    #[test]
    fn test_decode_url_in_code() {
        assert_eq!(
            decode_url_in_code(
                "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94",
                false
            ),
            ("https://www.baidu.com/s?ie=UTF-8&wd=天气".into(), true)
        );
        assert_eq!(
            decode_url_in_code(
                "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94天气",
                false
            ),
            ("https://www.baidu.com/s?ie=UTF-8&wd=天气天气".into(), true)
        );
        assert_eq!(
            decode_url_in_code(
                "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94)(",
                false
            ),
            ("https://www.baidu.com/s?ie=UTF-8&wd=天气)(".into(), true)
        );
        assert_eq!(
            decode_url_in_code(
                r#""https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94""#,
                false
            ),
            (r#""https://www.baidu.com/s?ie=UTF-8&wd=天气""#.into(), true)
        );
        // escape space
        assert_eq!(
            decode_url_in_code(
                "https://osu.ppy.sh/beatmapsets?q=malody%204k%20extra%20dan%20v3%E4%B8%AD",
                true
            ),
            (
                "https://osu.ppy.sh/beatmapsets?q=malody%204k%20extra%20dan%20v3中".into(),
                true
            )
        );
        // nothing happens
        assert_eq!(
            decode_url_in_code("https://osu.ppy.sh", true),
            ("https://osu.ppy.sh".into(), false)
        );
    }

    #[test]
    fn test_in_exclude() {
        let pattern = PathBuf::from("path/to/file.txt");

        // Case 1: Empty exclude should always return false
        let exclude: Vec<PathBuf> = Vec::new();
        assert!(!in_exclude(&exclude, &pattern));

        // Case 2: Single path in exclude that matches the pattern
        let exclude: Vec<PathBuf> = vec![PathBuf::from("path/to")];
        assert!(in_exclude(&exclude, &pattern));

        // Case 3: Single path in exclude that doesn't match the pattern
        let exclude: Vec<PathBuf> = vec![PathBuf::from("other/path")];
        assert!(!in_exclude(&exclude, &pattern));

        // Case 4: Multiple paths in exclude, one of them matches the pattern
        let exclude: Vec<PathBuf> = vec![PathBuf::from("path/to"), PathBuf::from("some/other")];
        assert!(in_exclude(&exclude, &pattern));

        // Case 5: Multiple paths in exclude, none of them matches the pattern
        let exclude: Vec<PathBuf> = vec![PathBuf::from("/other/path"), PathBuf::from("some/other")];
        assert!(!in_exclude(&exclude, &pattern));

        // Case 6: Do not except files that only match prefix
        let exclude: Vec<PathBuf> = vec![PathBuf::from("fi")];
        let pattern = PathBuf::from("file.txt");
        assert!(!in_exclude(&exclude, &pattern));
    }

    #[tokio::test]
    async fn exclude_and_recursive_test() {
        let test_str = "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94";
        let temp = TempDir::default();
        let test_path = PathBuf::from(temp.as_ref());
        let t1 = test_path.join("test1.txt");
        let mut t2 = test_path.join("test2");
        fs::create_dir(&t2).await.unwrap();
        t2 = t2.join("test2.txt");
        let t3 = test_path.join("exclude.txt");
        fs::write(&t1, test_str).await.unwrap();
        fs::write(&t2, test_str).await.unwrap();
        fs::write(&t3, test_str).await.unwrap();

        process_directory(
            vec![test_path.join("**/*")],
            vec![test_path.join("exclude.txt")],
            false,
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(
            fs::read_to_string(t1).await.unwrap(),
            decode(test_str).unwrap().clone()
        );
        assert_eq!(
            fs::read_to_string(t2).await.unwrap(),
            decode(test_str).unwrap().clone()
        );
        assert_eq!(fs::read_to_string(t3).await.unwrap(), test_str);
    }

    /// Returns the expected decode result.
    async fn write_test_file(path: &Path, delimiter: &str) -> String {
        let content = [
            "http://test.com/?q=天气",
            "testhttp://test.com/?q=%E5%A4%A9%E6%B0%94test",
        ];
        let content = content.join(delimiter);
        fs::write(path, content).await.unwrap();
        [
            "http://test.com/?q=天气",
            delimiter,
            "testhttp://test.com/?q=天气test",
        ]
        .concat()
    }

    #[tokio::test]
    async fn test_lf_crlf() {
        let temp_dir = TempDir::default();
        let crlf = temp_dir.join("crlf.txt");
        let crlf_expect = write_test_file(crlf.as_path(), "\r\n").await;

        let lf = temp_dir.join("lf.txt");
        let lf_expect = write_test_file(lf.as_path(), "\n").await;

        process_directory(vec![temp_dir.join("**/*")], vec![], false, false, false)
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(crlf).await.unwrap(), crlf_expect);
        assert_eq!(fs::read_to_string(lf).await.unwrap(), lf_expect);

        drop(temp_dir);
    }
}
