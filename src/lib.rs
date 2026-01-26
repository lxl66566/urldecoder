#![warn(clippy::cargo)]

pub mod error;
pub mod log;

#[cfg(feature = "verbose-log")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::Path,
    sync::OnceLock,
};

pub use error::*;
use memchr::{memchr, memmem::Finder};
use snafu::ResultExt;

#[cfg(feature = "verbose-log")]
use crate::log::VerboseLogger;
use crate::log::{DecodeLogger, NoOpLogger};

const SMALL_FILE_THRESHOLD: u64 = 256 * 1024;
const IO_BUF_SIZE: usize = 64 * 1024;
const URL_CHAR_BITMAP: [u32; 8] = gen_url_bitmap(b"-+&@#/%?=~_|!:,.;");
const URL_END_CHAR_BITMAP: [u32; 8] = gen_url_bitmap(b"-+&@#/%=~_|");
const HEX_MAP: [u8; 256] = gen_hex_map();
const HEX_INVALID: u8 = 0xFF;

const fn gen_url_bitmap(symbols: &[u8]) -> [u32; 8] {
    let mut bitmap = [0u32; 8];
    let mut c = b'0';
    while c <= b'9' {
        let idx = c as usize;
        bitmap[idx >> 5] |= 1u32 << (idx & 31);
        c += 1;
    }
    let mut c = b'A';
    while c <= b'Z' {
        let idx = c as usize;
        bitmap[idx >> 5] |= 1u32 << (idx & 31);
        c += 1;
    }
    let mut c = b'a';
    while c <= b'z' {
        let idx = c as usize;
        bitmap[idx >> 5] |= 1u32 << (idx & 31);
        c += 1;
    }
    let mut i = 0;
    while i < symbols.len() {
        let idx = symbols[i] as usize;
        bitmap[idx >> 5] |= 1u32 << (idx & 31);
        i += 1;
    }
    bitmap
}

const fn gen_hex_map() -> [u8; 256] {
    let mut map = [HEX_INVALID; 256];
    let mut i = 0;
    while i < 10 {
        map[(b'0' + i) as usize] = i;
        i += 1;
    }
    let mut i = 0;
    while i < 6 {
        map[(b'a' + i) as usize] = 10 + i;
        map[(b'A' + i) as usize] = 10 + i;
        i += 1;
    }
    map
}

#[inline(always)]
fn is_url_char(byte: u8) -> bool {
    let idx = byte as usize;
    unsafe { (URL_CHAR_BITMAP.get_unchecked(idx >> 5) >> (idx & 31)) & 1 == 1 }
}

#[inline(always)]
fn is_url_end_char(byte: u8) -> bool {
    let idx = byte as usize;
    unsafe { (URL_END_CHAR_BITMAP.get_unchecked(idx >> 5) >> (idx & 31)) & 1 == 1 }
}

#[inline]
#[cold]
fn cold() {}

#[inline]
fn likely(b: bool) -> bool {
    if !b {
        cold()
    }
    b
}

#[inline(always)]
fn trim_url_end(slice: &[u8]) -> (&[u8], &[u8]) {
    let mut end = slice.len();
    while end > 0 {
        if is_url_end_char(unsafe { *slice.get_unchecked(end - 1) }) {
            break;
        }
        end -= 1;
    }
    unsafe { (slice.get_unchecked(..end), slice.get_unchecked(end..)) }
}

fn http_finder() -> &'static Finder<'static> {
    static FINDER: OnceLock<Finder<'static>> = OnceLock::new();
    FINDER.get_or_init(|| Finder::new(b"http"))
}

// ============================================================================
// Core Logic
// ============================================================================

/// Directly handle complete memory slices and stream write results to Writer
/// This design combines the advantages of zero-copy mmap and stream output with
/// O(1) memory usage
pub fn decode_slice_to_writer<W: Write>(
    input: &[u8],
    writer: &mut W,
    escape_space: bool,
    #[cfg(feature = "verbose-log")] logger: &mut impl DecodeLogger,
) -> io::Result<bool> {
    let mut pos = 0;
    let len = input.len();
    let mut changed = false;
    let finder = http_finder();

    while pos < len {
        if let Some(match_idx) = finder.find(&input[pos..]) {
            let start = pos + match_idx;

            let is_http = input[start..].starts_with(b"http://");
            let is_https = input[start..].starts_with(b"https://");

            if is_http || is_https {
                // Write plain text before URL
                if start > pos {
                    writer.write_all(&input[pos..start])?;
                }

                // Find URL end
                let prefix_len = if is_https { 8 } else { 7 };
                let mut end = start + prefix_len;
                while end < len && is_url_char(input[end]) {
                    end += 1;
                }

                let raw_url = &input[start..end];
                let (valid_url, suffix) = trim_url_end(raw_url);

                // Decode URL and write directly
                #[cfg(feature = "verbose-log")]
                let url_changed = decode_url_to_writer(valid_url, writer, escape_space, logger)?;
                #[cfg(not(feature = "verbose-log"))]
                let url_changed = decode_url_to_writer(valid_url, writer, escape_space)?;
                if url_changed {
                    changed = true;
                }

                // Write suffix after trimmed punctuation
                if !suffix.is_empty() {
                    writer.write_all(suffix)?;
                }

                pos = end;
            } else {
                // find `http` but not a url
                writer.write_all(&input[pos..start + 4])?;
                pos = start + 4;
            }
        } else {
            // write all
            if pos < len {
                writer.write_all(&input[pos..])?;
            }
            break;
        }
    }
    Ok(changed)
}

#[inline(always)]
pub fn decode_url_to_writer<W: Write>(
    url: &[u8],
    writer: &mut W,
    escape_space: bool,
    #[cfg(feature = "verbose-log")] logger: &mut impl DecodeLogger,
) -> io::Result<bool> {
    #[cfg(not(feature = "verbose-log"))]
    let mut logger = NoOpLogger;
    logger.clear();

    let first_pct = match memchr(b'%', url) {
        Some(idx) => idx,
        None => {
            writer.write_all(url)?;
            logger.log_orig_slice(url);
            logger.log_res_slice(url);
            return Ok(false);
        }
    };

    if first_pct > 0 {
        writer.write_all(&url[..first_pct])?;
        logger.log_orig_slice(&url[..first_pct]);
        logger.log_res_slice(&url[..first_pct]);
    }

    let mut i = first_pct;
    let len = url.len();
    let mut changed = false;

    while i < len {
        if url[i] == b'%' && i + 2 < len {
            let h1 = url[i + 1];
            let h2 = url[i + 2];

            let v1 = unsafe { *HEX_MAP.get_unchecked(h1 as usize) };
            let v2 = unsafe { *HEX_MAP.get_unchecked(h2 as usize) };

            if likely((v1 | v2) != HEX_INVALID) {
                let decoded = (v1 << 4) | v2;
                if decoded == b' ' && escape_space {
                    writer.write_all(b"%20")?;
                    logger.log_orig_slice(b"%20");
                    logger.log_res_slice(b"%20");
                } else {
                    writer.write_all(&[decoded])?;
                    changed = true;
                    logger.log_orig(b'%');
                    logger.log_orig(h1);
                    logger.log_orig(h2);
                    logger.log_res(decoded);
                }
                i += 3;
                continue;
            }
        }

        writer.write_all(&[url[i]])?;
        logger.log_orig(url[i]);
        logger.log_res(url[i]);
        i += 1;
    }

    logger.print_if_changed(changed);
    Ok(changed)
}

// ============================================================================
// Public API
// ============================================================================

macro_rules! do_decode {
    ($input:expr, $writer:expr, $escape_space:expr, $verbose:expr) => {{
        if $verbose {
            #[cfg(feature = "verbose-log")]
            {
                let mut logger = VerboseLogger::new();
                decode_slice_to_writer($input, $writer, $escape_space, &mut logger)
            }
            #[cfg(not(feature = "verbose-log"))]
            {
                decode_slice_to_writer($input, $writer, $escape_space)
            }
        } else {
            #[cfg(feature = "verbose-log")]
            {
                let mut logger = NoOpLogger;
                decode_slice_to_writer($input, $writer, $escape_space, &mut logger)
            }
            #[cfg(not(feature = "verbose-log"))]
            {
                decode_slice_to_writer($input, $writer, $escape_space)
            }
        }
    }};
}

/// Decode String
pub fn decode_str(
    input: &str,
    escape_space: bool,
    #[cfg(feature = "verbose-log")] verbose: bool,
) -> Result<(String, bool)> {
    #[cfg(not(feature = "verbose-log"))]
    let verbose = false;

    let mut buf = Vec::with_capacity(input.len());
    let changed =
        do_decode!(input.as_bytes(), &mut buf, escape_space, verbose).context(WriteOutputSnafu)?;
    Ok((
        simdutf8::basic::from_utf8(&buf)
            .context(InvalidUtf8Snafu)?
            .to_owned(),
        changed,
    ))
}

/// Decode file and overwrite.
/// Use mmap zero-copy read, and stream output to temporary file (large files
/// only).
pub fn decode_file(
    path: impl AsRef<Path>,
    escape_space: bool,
    dry_run: bool,
    #[cfg(feature = "verbose-log")] verbose: bool,
    #[cfg(feature = "verbose-log")] p_counter: &AtomicUsize,
    #[cfg(feature = "verbose-log")] c_counter: &AtomicUsize,
) -> Result<()> {
    #[cfg(not(feature = "verbose-log"))]
    let verbose = false;

    let path = path.as_ref();
    let file = File::open(path).context(OpenInputSnafu { path })?;
    let metadata = file.metadata().context(ReadInputSnafu)?;
    let file_len = metadata.len();

    if file_len == 0 {
        #[cfg(feature = "verbose-log")]
        p_counter.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    let mmap = unsafe {
        memmap2::MmapOptions::new()
            .map(&file)
            .context(ReadInputSnafu)?
    };
    #[allow(unused)]
    let mut changed = false;

    #[allow(unused)]
    if dry_run {
        let mut sink = io::sink();
        changed = do_decode!(&mmap, &mut sink, escape_space, verbose).context(WriteOutputSnafu)?;
    } else if file_len < SMALL_FILE_THRESHOLD {
        // decode to memory and overwrite
        let mut buffer = Vec::with_capacity(file_len as usize);
        changed =
            do_decode!(&mmap, &mut buffer, escape_space, verbose).context(WriteOutputSnafu)?;
        drop(mmap);
        drop(file);
        if changed {
            fs::write(path, &buffer).context(WriteBackSnafu { path })?;
        }
    } else {
        // decode to temporary file
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut temp_file = tempfile::Builder::new()
            .prefix(".tmp_processing_")
            .tempfile_in(parent)
            .context(CreateTempSnafu { dir: parent })?;
        let _ = temp_file.as_file().set_permissions(metadata.permissions());
        {
            let mut buf_writer = BufWriter::with_capacity(IO_BUF_SIZE, &mut temp_file);
            changed = do_decode!(&mmap, &mut buf_writer, escape_space, verbose)
                .context(WriteOutputSnafu)?;
            buf_writer.flush().context(WriteOutputSnafu)?;
        }
        drop(mmap);
        drop(file);

        if changed {
            temp_file.persist(path).context(PersistTempSnafu { path })?;
        }
    }

    #[cfg(feature = "verbose-log")]
    {
        p_counter.fetch_add(1, Ordering::Relaxed);
        if changed {
            c_counter.fetch_add(1, Ordering::Relaxed);
            if verbose {
                println!("Processed File: {:?}", path);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn test_basic() {
        // basic
        assert_eq!(
            decode_str(
                "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94",
                false,
                #[cfg(feature = "verbose-log")]
                false
            )
            .unwrap(),
            ("https://www.baidu.com/s?ie=UTF-8&wd=天气".into(), true)
        );
        // symbol end
        assert_eq!(
            decode_str(
                "(https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94)",
                false,
                #[cfg(feature = "verbose-log")]
                false
            )
            .unwrap(),
            ("(https://www.baidu.com/s?ie=UTF-8&wd=天气)".into(), true)
        );
        // escape space
        assert_eq!(
            decode_str(
                "https://osu.ppy.sh/beatmapsets?q=malody%204k%20extra%20dan%20v3%E4%B8%AD",
                true,
                #[cfg(feature = "verbose-log")]
                true
            )
            .unwrap(),
            (
                "https://osu.ppy.sh/beatmapsets?q=malody%204k%20extra%20dan%20v3中".into(),
                true
            )
        );
        // nothing happens
        assert_eq!(
            decode_str(
                "https://osu.ppy.sh",
                true,
                #[cfg(feature = "verbose-log")]
                false
            )
            .unwrap(),
            ("https://osu.ppy.sh".into(), false)
        );
    }

    #[test]
    fn test_long_url() {
        let mut url = "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94".to_string();
        for _ in 0..10000 {
            url.push_str("%20");
        }
        assert_eq!(
            decode_str(
                &url,
                false,
                #[cfg(feature = "verbose-log")]
                false
            )
            .unwrap(),
            (
                "https://www.baidu.com/s?ie=UTF-8&wd=天气".to_string() + " ".repeat(10000).as_str(),
                true
            )
        );

        let base = "a".repeat(60000);
        assert_eq!(
            decode_str(
                &(base.clone() + &url),
                false,
                #[cfg(feature = "verbose-log")]
                false
            )
            .unwrap(),
            (
                (base + "https://www.baidu.com/s?ie=UTF-8&wd=天气") + " ".repeat(10000).as_str(),
                true
            )
        )
    }

    #[test]
    fn test_decode_file() {
        let temp = NamedTempFile::new().unwrap();
        let t1 = temp.path().to_path_buf();
        let test_str = "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94";
        fs::write(&t1, test_str).unwrap();

        decode_file(
            &t1,
            false,
            false,
            #[cfg(feature = "verbose-log")]
            false,
            #[cfg(feature = "verbose-log")]
            &AtomicUsize::new(0),
            #[cfg(feature = "verbose-log")]
            &AtomicUsize::new(0),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(t1).unwrap(),
            "https://www.baidu.com/s?ie=UTF-8&wd=天气"
        );
    }
}
