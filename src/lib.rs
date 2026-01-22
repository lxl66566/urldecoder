#![warn(clippy::cargo)]

use std::{
    cell::RefCell,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(feature = "color")]
use colored::Colorize;
use memchr::memchr;
use snafu::{ResultExt, Snafu};

// ============================================================================
// Error Definitions (Snafu)
// ============================================================================

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to open input file {}: {}", path.display(), source))]
    OpenInput { path: PathBuf, source: io::Error },

    #[snafu(display("Failed to read input data: {}", source))]
    ReadInput { source: io::Error },

    #[snafu(display("Failed to decode: {}", source))]
    Decode { source: io::Error },

    #[snafu(display("Failed to write output data: {}", source))]
    WriteOutput { source: io::Error },

    #[snafu(display("Failed to create temporary file in {}: {}", dir.display(), source))]
    CreateTemp { dir: PathBuf, source: io::Error },

    #[snafu(display("Failed to persist temporary file to {}: {}", path.display(), source))]
    PersistTemp {
        path: PathBuf,
        source: tempfile::PersistError,
    },

    #[snafu(display("Failed to write back to original file {}: {}", path.display(), source))]
    WriteBack { path: PathBuf, source: io::Error },

    #[snafu(display("Invalid UTF-8 sequence: {}", source))]
    InvalidUtf8 { source: std::string::FromUtf8Error },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

// ============================================================================
// Constants & Lookups
// ============================================================================

const SMALL_FILE_THRESHOLD: u64 = 1024 * 1024;
const IO_BUF_SIZE: usize = 64 * 1024;

// Logging constants are only needed if the feature is enabled
#[cfg(feature = "verbose-log")]
const LOG_RES_CAPACITY: usize = 256;
#[cfg(feature = "verbose-log")]
const LOG_ORIG_CAPACITY: usize = LOG_RES_CAPACITY * 3;

thread_local! {
    /// Reusable IO buffer to avoid 64KB allocation per file.
    /// This is always kept as it improves IO performance regardless of logging.
    static IO_BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; IO_BUF_SIZE]);

    /// Reusable buffer for the decoded result logging.
    #[cfg(feature = "verbose-log")]
    static LOG_RES_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(LOG_RES_CAPACITY));

    /// Reusable buffer for the original URL logging.
    #[cfg(feature = "verbose-log")]
    static LOG_ORIG_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(LOG_ORIG_CAPACITY));
}

const URL_CHAR: [bool; 256] = gen_url_map(b"-+&@#/%?=~_|!:,.;");
const URL_END_CHAR: [bool; 256] = gen_url_map(b"-+&@#/%=~_|");
const HEX_MAP: [u8; 256] = gen_hex_map();

const fn gen_url_map(symbols: &[u8]) -> [bool; 256] {
    let mut map = [false; 256];
    let mut c = b'0';
    while c <= b'9' {
        map[c as usize] = true;
        c += 1;
    }
    let mut c = b'A';
    while c <= b'Z' {
        map[c as usize] = true;
        c += 1;
    }
    let mut c = b'a';
    while c <= b'z' {
        map[c as usize] = true;
        c += 1;
    }
    let mut i = 0;
    while i < symbols.len() {
        map[symbols[i] as usize] = true;
        i += 1;
    }
    map
}

const fn gen_hex_map() -> [u8; 256] {
    let mut map = [255; 256];

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
fn is_hex(c: u8) -> bool {
    HEX_MAP[c as usize] != 255
}

#[inline(always)]
fn from_hex(c: u8) -> u8 {
    HEX_MAP[c as usize]
}

// ============================================================================
// Core Logic
// ============================================================================

/// Decodes a URL slice, writes to writer.
/// Logging logic is conditionally compiled.
fn decode_chunk<W: Write>(
    url_bytes: &[u8],
    writer: &mut W,
    escape_space: bool,
    _verbose: bool, // Prefix with _ to avoid unused warning when feature is disabled
) -> io::Result<bool> {
    let mut i = 0;
    let len = url_bytes.len();
    let mut changed = false;

    // ------------------------------------------------------------------------
    // Conditional Compilation Macros for Logging
    // ------------------------------------------------------------------------
    #[cfg(feature = "verbose-log")]
    macro_rules! init_log {
        () => {
            if _verbose {
                LOG_RES_BUF.with(|b| b.borrow_mut().clear());
                LOG_ORIG_BUF.with(|b| b.borrow_mut().clear());
            }
        };
    }
    #[cfg(not(feature = "verbose-log"))]
    macro_rules! init_log {
        () => {};
    }

    #[cfg(feature = "verbose-log")]
    macro_rules! log_orig {
        ($b:expr) => {
            if _verbose {
                LOG_ORIG_BUF.with(|buf| push_limit(&mut buf.borrow_mut(), $b, LOG_ORIG_CAPACITY));
            }
        };
    }
    #[cfg(not(feature = "verbose-log"))]
    macro_rules! log_orig {
        ($b:expr) => {};
    }

    #[cfg(feature = "verbose-log")]
    macro_rules! log_res {
        ($b:expr) => {
            if _verbose {
                LOG_RES_BUF.with(|buf| push_limit(&mut buf.borrow_mut(), $b, LOG_RES_CAPACITY));
            }
        };
    }
    #[cfg(not(feature = "verbose-log"))]
    macro_rules! log_res {
        ($b:expr) => {};
    }

    #[cfg(feature = "verbose-log")]
    macro_rules! print_log {
        () => {
            if _verbose && changed {
                LOG_ORIG_BUF.with(|orig_cell| {
                    LOG_RES_BUF.with(|res_cell| {
                        let orig = orig_cell.borrow();
                        let res = res_cell.borrow();
                        let orig_s = String::from_utf8_lossy(&orig);
                        let res_s = String::from_utf8_lossy(&res);
                        let orig_suffix = if orig.len() == LOG_ORIG_CAPACITY {
                            "..."
                        } else {
                            ""
                        };
                        let res_suffix = if res.len() == LOG_RES_CAPACITY {
                            "..."
                        } else {
                            ""
                        };
                        #[cfg(feature = "color")]
                        {
                            println!("{}", format!("- {}{}", orig_s, orig_suffix).red());
                            println!("{}", format!("+ {}{}", res_s, res_suffix).green());
                        }
                        #[cfg(not(feature = "color"))]
                        {
                            println!("- {}{}\n+ {}{}", orig_s, orig_suffix, res_s, res_suffix);
                        }
                    })
                });
            }
        };
    }
    #[cfg(not(feature = "verbose-log"))]
    macro_rules! print_log {
        () => {};
    }
    // ------------------------------------------------------------------------

    init_log!();

    while i < len {
        let b = url_bytes[i];
        if b == b'%' && i + 2 < len {
            let h1 = url_bytes[i + 1];
            let h2 = url_bytes[i + 2];

            if is_hex(h1) && is_hex(h2) {
                let decoded_byte = (from_hex(h1) << 4) | from_hex(h2);

                if escape_space && decoded_byte == b' ' {
                    writer.write_all(b"%20")?;
                    log_orig!(b'%');
                    log_orig!(b'2');
                    log_orig!(b'0');
                    log_res!(b'%');
                    log_res!(b'2');
                    log_res!(b'0');
                } else {
                    writer.write_all(&[decoded_byte])?;
                    log_orig!(b'%');
                    log_orig!(h1);
                    log_orig!(h2);
                    log_res!(decoded_byte);
                    changed = true;
                }
                i += 3;
                continue;
            }
        }

        writer.write_all(&[b])?;
        log_orig!(b);
        log_res!(b);
        i += 1;
    }

    print_log!();

    Ok(changed)
}

#[cfg(feature = "verbose-log")]
#[inline]
fn push_limit(vec: &mut Vec<u8>, byte: u8, limit: usize) {
    if vec.len() < limit {
        vec.push(byte);
    }
}

/// Decodes the urls in the stream, writes the result to writer.
///
/// # Arguments
///
/// * `reader` - The reader to read the stream from.
/// * `writer` - The writer to write the decoded stream to.
/// * `escape_space` - Whether to decode `%20` to space.
/// * `verbose` - Whether to print verbose logs. (needs `verbose-log` feature)
///
/// # Returns
///
/// (number of processed bytes, whether the decode happened)
pub fn decode_stream<R, W>(
    mut reader: R,
    mut writer: W,
    escape_space: bool,
    verbose: bool,
) -> Result<(u64, bool)>
where
    R: Read,
    W: Write,
{
    IO_BUF.with(|cell| {
        let mut buf_guard = cell.borrow_mut();
        let buf = buf_guard.as_mut_slice();

        let mut offset = 0;
        let mut len = 0;
        let mut total_processed = 0u64;
        let mut has_changes = false;

        let mut in_url = false;
        let mut url_start_idx = 0;

        loop {
            let n = reader.read(&mut buf[len..]).context(ReadInputSnafu)?;
            if n == 0 {
                // EOF: 处理剩余数据
                if len > 0 {
                    if in_url {
                        let url_slice = &buf[url_start_idx..len];
                        let (valid_url, suffix) = trim_url_end(url_slice);

                        if decode_chunk(valid_url, &mut writer, escape_space, verbose)
                            .context(DecodeSnafu)?
                        {
                            has_changes = true;
                        }
                        writer.write_all(suffix).context(WriteOutputSnafu)?;
                    } else {
                        writer
                            .write_all(&buf[offset..len])
                            .context(WriteOutputSnafu)?;
                    }
                    total_processed += (len - offset) as u64;
                }
                break;
            }
            len += n;

            let mut pos = offset;

            while pos < len {
                if !in_url {
                    let search_slice = &buf[pos..len];
                    match memchr(b'h', search_slice) {
                        Some(rel_idx) => {
                            let h_idx = pos + rel_idx;
                            if h_idx > offset {
                                writer
                                    .write_all(&buf[offset..h_idx])
                                    .context(WriteOutputSnafu)?;
                                total_processed += (h_idx - offset) as u64;
                            }

                            if let Some(prefix_len) = check_url_prefix(&buf[h_idx..len]) {
                                in_url = true;
                                url_start_idx = h_idx;
                                offset = h_idx;
                                pos = h_idx + prefix_len;
                            } else if len - h_idx < 8 {
                                offset = h_idx;
                                pos = len;
                            } else {
                                pos = h_idx + 1;
                            }
                        }
                        None => {
                            writer
                                .write_all(&buf[offset..len])
                                .context(WriteOutputSnafu)?;
                            total_processed += (len - offset) as u64;
                            offset = len;
                            pos = len;
                        }
                    }
                } else {
                    let mut end_found = false;
                    while pos < len {
                        if !URL_CHAR[buf[pos] as usize] {
                            end_found = true;
                            break;
                        }
                        pos += 1;
                    }

                    if end_found {
                        let raw_url_slice = &buf[url_start_idx..pos];
                        let (valid_url, suffix) = trim_url_end(raw_url_slice);
                        if decode_chunk(valid_url, &mut writer, escape_space, verbose)
                            .context(DecodeSnafu)?
                        {
                            has_changes = true;
                        }
                        if !suffix.is_empty() {
                            writer.write_all(suffix).context(WriteOutputSnafu)?;
                        }

                        let processed_len = pos - url_start_idx;
                        total_processed += processed_len as u64;

                        in_url = false;
                        offset = pos;
                    } else {
                        break;
                    }
                }
            }

            if offset < len {
                if offset == 0 && len == buf.len() {
                    if in_url {
                        decode_chunk(&buf[..len], &mut writer, escape_space, verbose)
                            .context(DecodeSnafu)?;
                        in_url = false;
                    } else {
                        writer.write_all(&buf[..len]).context(WriteOutputSnafu)?;
                    }
                    total_processed += len as u64;
                    len = 0;
                    offset = 0;
                } else {
                    let remaining = len - offset;
                    buf.copy_within(offset..len, 0);
                    len = remaining;

                    if in_url {
                        url_start_idx -= offset;
                    }
                    offset = 0;
                }
            } else {
                len = 0;
                offset = 0;
            }
        }

        Ok((total_processed, has_changes))
    })
}

#[inline]
fn check_url_prefix(slice: &[u8]) -> Option<usize> {
    if slice.len() >= 7 && slice.starts_with(b"http://") {
        Some(7)
    } else if slice.len() >= 8 && slice.starts_with(b"https://") {
        Some(8)
    } else {
        None
    }
}

#[inline]
fn trim_url_end(slice: &[u8]) -> (&[u8], &[u8]) {
    let mut end = slice.len();
    while end > 0 {
        if URL_END_CHAR[slice[end - 1] as usize] {
            break;
        }
        end -= 1;
    }
    (&slice[..end], &slice[end..])
}

/// Decodes a string, returns the decoded string and whether the decode
/// happened.
///
/// # Arguments
///
/// * `input` - The string to decode.
/// * `escape_space` - Whether to decode `%20` to space.
/// * `verbose` - Whether to print verbose logs. (needs `verbose-log` feature)
///
/// # Returns
///
/// (decoded string, whether the decode happened)
pub fn decode_str(input: &str, escape_space: bool, verbose: bool) -> Result<(String, bool)> {
    let mut buf = Vec::new();
    let changed = {
        let mut writer = io::BufWriter::new(&mut buf);
        let (_, changed) = decode_stream(input.as_bytes(), &mut writer, escape_space, verbose)?;
        changed
    };
    Ok((String::from_utf8(buf).context(InvalidUtf8Snafu)?, changed))
}

/// Decodes a file and overwrites it if `dry_run` is false.
/// Note that for big files (> 1MB), this function will use a temporary file
/// to avoid too much memory allocation.
///
/// # Arguments
///
/// * `path` - The path to the file to decode.
/// * `escape_space` - Whether to decode `%20` to space.
/// * `verbose` - Whether to print verbose logs. (needs `verbose-log` feature)
/// * `dry_run` - Whether to print the result without overwriting the file.
/// * `p_counter` - The counter for processed files.
/// * `c_counter` - The counter for changed files.
pub fn decode_file(
    path: &Path,
    escape_space: bool,
    verbose: bool,
    dry_run: bool,
    p_counter: &AtomicUsize,
    c_counter: &AtomicUsize,
) -> Result<()> {
    let file = File::open(path).context(OpenInputSnafu { path })?;
    let metadata = file.metadata().context(ReadInputSnafu)?;
    let file_len = metadata.len();
    let reader = BufReader::new(file);

    let (_processed_bytes, changed) = if dry_run {
        decode_stream(reader, io::sink(), escape_space, verbose)?
    } else if file_len < SMALL_FILE_THRESHOLD {
        let mut buffer = Vec::with_capacity(file_len as usize);
        let res = decode_stream(reader, &mut buffer, escape_space, verbose)?;
        if res.1 {
            fs::write(path, &buffer).context(WriteBackSnafu { path })?;
        }
        res
    } else {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut temp_file = tempfile::Builder::new()
            .prefix(".tmp_processing_")
            .tempfile_in(parent)
            .context(CreateTempSnafu { dir: parent })?;

        // Ignore permission errors silently or log them, but don't fail the whole
        // process
        let _ = temp_file.as_file().set_permissions(metadata.permissions());

        let res = {
            let mut writer = BufWriter::new(&mut temp_file);
            let res = decode_stream(reader, &mut writer, escape_space, verbose)?;
            writer.flush().context(WriteOutputSnafu)?;
            res
        };

        if res.1 {
            temp_file.persist(path).context(PersistTempSnafu { path })?;
        }
        res
    };

    p_counter.fetch_add(1, Ordering::Relaxed);

    if changed {
        c_counter.fetch_add(1, Ordering::Relaxed);
        // Only print if feature is enabled AND verbose is true
        #[cfg(feature = "verbose-log")]
        if verbose {
            println!("Processed File: {:?}", path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_basic() {
        // basic
        assert_eq!(
            decode_str(
                "https://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94",
                false,
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
            decode_str("https://osu.ppy.sh", true, false).unwrap(),
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
            decode_str(&url, false, false).unwrap(),
            (
                "https://www.baidu.com/s?ie=UTF-8&wd=天气".to_string() + " ".repeat(10000).as_str(),
                true
            )
        );

        let base = "a".repeat(60000);
        assert_eq!(
            decode_str(&(base.clone() + &url), false, false).unwrap(),
            (
                (base + "https://www.baidu.com/s?ie=UTF-8&wd=天气") + " ".repeat(10000).as_str(),
                true
            )
        )
    }
}
