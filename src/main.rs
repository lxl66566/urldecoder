use clap::Parser;
use colored::Colorize;
use glob::glob;
use regex::Regex;
use std::path::PathBuf;
use std::{borrow::Cow, fs, io};
use urlencoding::decode;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(author, version, about, long_about = None, after_help = r#"Examples:
urldecoder test/t.md    # decode test/t.md
urldecoder *.md         # decode all markdown files in current folder
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
}

/// Find all urls in the code and decode them.
/// Returns the String of decoded code and a bool indicates whether the code has decoded urls.
fn decode_url_in_code(code: &str) -> (String, bool) {
    let mut replaced = false;
    let regex =
        Regex::new(r#"https?://[-A-Za-z0-9+&@#/%?=~_|!:,.;]+[-A-Za-z0-9+&@#/%=~_|]"#).unwrap();
    (
        regex
            .replace_all(code, |caps: &regex::Captures| {
                let url = &caps[0];
                let decoded_url = decode(url).unwrap_or(Cow::Borrowed(url));
                if url == decoded_url {
                    return url.to_string();
                }
                replaced = true;
                decoded_url.into_owned()
            })
            .into_owned(),
        replaced,
    )
}

fn process_file(file_path: &PathBuf, args: &Cli) -> io::Result<()> {
    let mut replaced = false;
    let content = fs::read_to_string(file_path)?;
    let mut decoded_content = String::new();
    for line in content.lines() {
        let (decoded_line, replaced_line) = decode_url_in_code(line);
        if replaced_line {
            if !replaced {
                println!("In file: {}", file_path.display());
                replaced = true;
            }
            println!(
                "{}\n{}",
                format!("- {}", line).red(),
                format!("+ {}", decoded_line).green()
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
        let entry = entry?;
        if let Err(err) = process_file(&entry, args) {
            if args.verbose || err.kind() != io::ErrorKind::InvalidData {
                eprintln!("ERROR: {} : {}", err, &entry.display())
            };
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Cli::parse();
    // if let Err(err) = process_directory(&args) {
    //     eprintln!("Error: {}", err);
    // }
    process_directory(&args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_decode_url_in_code() {
        assert_eq!(
            decode_url_in_code("https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94"),
            ("https://www.baidu.com/s?ie=UTF-8&wd=天气".into(), true)
        );
        assert_eq!(
            decode_url_in_code("https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94天气"),
            ("https://www.baidu.com/s?ie=UTF-8&wd=天气天气".into(), true)
        );
        assert_eq!(
            decode_url_in_code("https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94)("),
            ("https://www.baidu.com/s?ie=UTF-8&wd=天气)(".into(), true)
        );
        assert_eq!(
            decode_url_in_code(r#""https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94""#),
            (r#""https://www.baidu.com/s?ie=UTF-8&wd=天气""#.into(), true)
        );
    }
}
