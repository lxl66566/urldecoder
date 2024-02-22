use clap::{ArgAction, Parser};
use colored::Colorize;
use glob::glob;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::{borrow::Cow, fs, io};
use urlencoding::decode;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(author, version, about, long_about = None, after_help = r#"Examples:
urldecoder test/t.md    # decode test/t.md
urldecoder *.md -e my   # decode all markdown files in current folder except which in `my` folder
urldecoder *            # decode all files in current folder
"#)]
pub struct Cli {
    /// Files to convert. It uses glob("**/{file}") to glob given pattern, like python's `rglob`
    file: PathBuf,
    /// Show result only without overwrite
    #[arg(short, long)]
    dry_run: bool,
    /// Show full error message
    #[arg(short, long)]
    verbose: bool,
    /// Exclude file or folder
    #[arg(short, long, action = ArgAction::Append)]
    exclude: Vec<PathBuf>,
    /// Do not decode `%20` to space
    #[arg(long)]
    escape_space: bool,
}

/// Find all urls in the code and decode them.
/// Returns the String of decoded code and a bool indicates whether the code has decoded urls.
fn decode_url_in_code(code: &str, escape_space: bool) -> (String, bool) {
    let mut replaced = false;
    let regex =
        Regex::new(r#"https?://[-A-Za-z0-9+&@#/%?=~_|!:,.;]+[-A-Za-z0-9+&@#/%=~_|]"#).unwrap();
    (
        regex
            .replace_all(code, |caps: &regex::Captures| {
                let url = &caps[0];
                let mut decoded_url = decode(url).unwrap_or(Cow::Borrowed(url)).into_owned();
                if escape_space {
                    // Replacing after decoding will not affect much performance (Benchmarked).
                    decoded_url = decoded_url.replace(' ', "%20");
                }
                if url == decoded_url {
                    return url.to_string();
                }
                replaced = true;
                decoded_url
            })
            .into_owned(),
        replaced,
    )
}

fn in_exclude<'a, T>(exclude: T, pattern: &'a Path) -> bool
where
    T: IntoIterator<Item = &'a PathBuf>,
{
    exclude.into_iter().any(|p| pattern.strip_prefix(p).is_ok())
}

fn process_file(file_path: &PathBuf, args: &Cli) -> io::Result<()> {
    let mut replaced = false;
    let content = fs::read_to_string(file_path)?;
    let mut decoded_content = String::new();
    for (line_number, line) in content.lines().enumerate() {
        let (decoded_line, replaced_line) = decode_url_in_code(line, args.escape_space);
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
        decoded_content.push('\n');
    }
    if replaced && !args.dry_run {
        fs::write(file_path, decoded_content)?;
    }
    Ok(())
}

fn process_directory(args: &Cli) -> Result<()> {
    for entry in glob(&format!("**/{}", args.file.display()))? {
        let entry = &entry?;
        if !entry.is_file() || in_exclude(&args.exclude, entry) {
            continue;
        }
        if let Err(err) = process_file(entry, args) {
            if args.verbose || err.kind() != io::ErrorKind::InvalidData {
                eprintln!("ERROR: {} : {}", err, entry.display())
            };
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args = Cli::parse();
    if args.exclude.is_empty() {
        args.exclude.push("node_modules".into());
    }
    process_directory(&args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
