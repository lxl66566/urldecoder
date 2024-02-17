use clap::Parser;
use colored::Colorize;
use glob::glob;
use regex::Regex;
use std::path::PathBuf;
use std::{borrow::Cow, fs, io};
use urlencoding::decode;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// File or folder to convert, defaults to the current directory.
    path: Option<PathBuf>,
    /// Show result only, no overwrite.
    #[arg(short, long)]
    dry_run: bool,
}

fn decode_url_in_code(code: &str) -> String {
    let regex = Regex::new(r#"https?://[^\s"]+"#).unwrap();
    regex
        .replace_all(code, |caps: &regex::Captures| {
            let url = &caps[0];
            let decoded_url = decode(url).unwrap_or(Cow::Borrowed(url));
            println!("`{}` => `{}`", url.red(), decoded_url.to_string().green());
            decoded_url.into_owned()
        })
        .into_owned()
}

fn process_file(file_path: PathBuf, args: &Cli) -> io::Result<()> {
    println!("Processing file: {}", file_path.display());
    let content = fs::read_to_string(&file_path)?;
    let decoded_content = decode_url_in_code(&content);
    if !args.dry_run {
        fs::write(&file_path, decoded_content)?;
    }

    Ok(())
}

fn process_directory(args: &Cli) -> Result<()> {
    let path = args.path.clone().unwrap_or(PathBuf::from("."));
    for entry in glob(&format!("{}/**/*", path.display()))? {
        if let Err(err) = process_file(entry?, args) {
            eprintln!("Error: {}", err);
        }
    }
    Ok(())
}

fn main() {
    let args = Cli::parse();
    if let Err(err) = process_directory(&args) {
        eprintln!("Error: {}", err);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_decode_url_in_code() {
        assert_eq!(
            decode_url_in_code("https://www.baidu.com/s?ie=UTF-8&wd=天气"),
            "https://www.baidu.com/s?ie=UTF-8&wd=天气"
        );
        assert_eq!(
            decode_url_in_code("https://www.baidu.com/s?ie=UTF-8&wd=天气天气"),
            "https://www.baidu.com/s?ie=UTF-8&wd=天气天气"
        );
        assert_eq!(
            decode_url_in_code("https://www.baidu.com/s?ie=UTF-8&wd=天气)("),
            "https://www.baidu.com/s?ie=UTF-8&wd=天气)("
        );
        assert_eq!(
            decode_url_in_code(r#""https://www.baidu.com/s?ie=UTF-8&wd=天气""#),
            r#""https://www.baidu.com/s?ie=UTF-8&wd=天气""#
        );
    }
}
