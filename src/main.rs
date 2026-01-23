#![cfg(feature = "bin")]
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use clap::{ArgAction, Parser};
use glob::glob;
use rayon::prelude::*;
use snafu::ResultExt;
use urldecoder::decode_file;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Files to process, allows wildcard pattern
    #[clap(required = true)]
    files: Vec<String>,

    /// Show result only, without overwrite
    #[arg(short, long)]
    dry_run: bool,

    /// Do not print decode result
    #[arg(short, long)]
    no_output: bool,

    /// Exclude file or folder by relative path prefix
    #[arg(short, long, action = ArgAction::Append)]
    exclude: Vec<PathBuf>,

    /// Do not decode `%20` to space
    #[arg(long)]
    escape_space: bool,
}

#[inline]
fn in_exclude(exclude: &[PathBuf], path: &Path) -> bool {
    exclude.iter().any(|p| path.starts_with(p) || path == p)
}

fn main() -> Result<(), snafu::Whatever> {
    let mut cli = Cli::parse();

    cli.exclude.push("node_modules".into());

    process_directory(
        cli.files,
        &cli.exclude,
        cli.escape_space,
        !cli.no_output,
        cli.dry_run,
    )?;

    Ok(())
}

fn process_directory(
    files: Vec<String>,
    exclude: &[PathBuf],
    escape_space: bool,
    verbose: bool,
    dry_run: bool,
) -> Result<(), snafu::Whatever> {
    let mut paths = Vec::new();
    for pattern in &files {
        for path in
            (glob(pattern).with_whatever_context(|e| format!("Glob pattern error: {e}"))?).flatten()
        {
            if path.is_file() && !in_exclude(exclude, &path) {
                paths.push(path);
            }
        }
    }

    if paths.is_empty() {
        println!("No files found.");
        return Ok(());
    }

    let processed_count = AtomicUsize::new(0);
    let changed_count = AtomicUsize::new(0);

    paths.par_iter().for_each(|path| {
        if let Err(e) = decode_file(
            path,
            escape_space,
            verbose,
            dry_run,
            &processed_count,
            &changed_count,
        ) {
            eprintln!("ERROR processing {}: {}", path.display(), e);
        }
    });

    println!(
        "Processed {} files, {} files changed.",
        processed_count.load(Ordering::Relaxed),
        changed_count.load(Ordering::Relaxed)
    );
    Ok(())
}

#[cfg(all(test, feature = "bin"))]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use urldecoder::decode_str;

    use super::*;

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

    #[test]
    fn exclude_and_recursive_test() {
        let test_str = "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94";
        let temp = TempDir::new().unwrap();
        let test_path = PathBuf::from(temp.as_ref());
        let t1 = test_path.join("test1.txt");
        let mut t2 = test_path.join("test2");
        fs::create_dir(&t2).unwrap();
        t2 = t2.join("test2.txt");
        let t3 = test_path.join("exclude.txt");
        fs::write(&t1, test_str).unwrap();
        fs::write(&t2, test_str).unwrap();
        fs::write(&t3, test_str).unwrap();

        process_directory(
            vec![test_path.join("**/*").to_string_lossy().to_string()],
            &[test_path.join("exclude.txt")],
            false,
            false,
            false,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(t1).unwrap(),
            decode_str(test_str, false, false).unwrap().0
        );
        assert_eq!(
            fs::read_to_string(t2).unwrap(),
            decode_str(test_str, false, false).unwrap().0
        );
        assert_eq!(fs::read_to_string(t3).unwrap(), test_str);
    }
}
