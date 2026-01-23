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

// Logging constants
#[cfg(feature = "verbose-log")]
const LOG_RES_CAPACITY: usize = 256;
#[cfg(feature = "verbose-log")]
const LOG_ORIG_CAPACITY: usize = LOG_RES_CAPACITY * 3;

thread_local! {
    /// Reusable IO buffer for reading input.
    static IO_BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; IO_BUF_SIZE]);

    /// Reusable output buffer to batch writes and allow SIMD optimizations.
    /// Capacity is doubled to ensure enough space for expansions if needed.
    static OUT_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(IO_BUF_SIZE * 2));

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

/// Decodes a URL slice, appends result to `out_vec`.
fn decode_chunk(
    url_bytes: &[u8],
    out_vec: &mut Vec<u8>,
    escape_space: bool,
    _verbose: bool,
) -> bool {
    let mut i = 0;
    let len = url_bytes.len();
    let mut changed = false;

    // Reserve space to avoid frequent reallocations
    out_vec.reserve(len);

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
                    out_vec.extend_from_slice(b"%20");
                    log_orig!(b'%');
                    log_orig!(b'2');
                    log_orig!(b'0');
                    log_res!(b'%');
                    log_res!(b'2');
                    log_res!(b'0');
                } else {
                    out_vec.push(decoded_byte);
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

        out_vec.push(b);
        log_orig!(b);
        log_res!(b);
        i += 1;
    }

    print_log!();

    changed
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
    IO_BUF.with(|io_cell| {
        OUT_BUF.with(|out_cell| {
            let mut buf_guard = io_cell.borrow_mut();
            let buf = buf_guard.as_mut_slice();

            let mut out_guard = out_cell.borrow_mut();
            let out = &mut *out_guard;

            let mut offset = 0; // Start of valid data in buf
            let mut len = 0; // End of valid data in buf
            let mut total_processed = 0u64;
            let mut has_changes = false;

            let mut in_url = false;
            let mut url_start_idx = 0;

            loop {
                // If there is leftover data, move it to the beginning of the buffer.
                // This is crucial for handling URLs that span across buffer reads.
                if offset > 0 && len > offset {
                    buf.copy_within(offset..len, 0);
                    len -= offset;
                    if in_url {
                        url_start_idx -= offset;
                    }
                    offset = 0;
                } else if offset == len {
                    // Buffer is fully processed
                    len = 0;
                    offset = 0;
                }

                // Fill the rest of the buffer
                let n = reader.read(&mut buf[len..]).context(ReadInputSnafu)?;
                if n == 0 {
                    // EOF
                    if len > 0 {
                        out.clear();
                        if in_url {
                            // Decode the remaining part of the URL
                            let url_slice = &buf[url_start_idx..len];
                            let (valid_url, suffix) = trim_url_end(url_slice);

                            if decode_chunk(valid_url, out, escape_space, verbose) {
                                has_changes = true;
                            }
                            writer.write_all(out).context(WriteOutputSnafu)?;
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
                        // Search for the next 'h'
                        match memchr(b'h', &buf[pos..len]) {
                            Some(rel_idx) => {
                                let h_idx = pos + rel_idx;

                                // Write data before 'h'
                                if h_idx > offset {
                                    writer
                                        .write_all(&buf[offset..h_idx])
                                        .context(WriteOutputSnafu)?;
                                    total_processed += (h_idx - offset) as u64;
                                    offset = h_idx;
                                }

                                if let Some(prefix_len) = check_url_prefix(&buf[h_idx..len]) {
                                    in_url = true;
                                    url_start_idx = h_idx;
                                    offset = h_idx; // Mark start, don't write yet
                                    pos = h_idx + prefix_len;
                                } else {
                                    // Boundary check: if we are near the end of buffer,
                                    // we might have a truncated "https://"
                                    if len - h_idx < 8 {
                                        offset = h_idx;
                                        pos = len; // Stop processing, wait for next read
                                    } else {
                                        pos = h_idx + 1;
                                    }
                                }
                            }
                            None => {
                                // No 'h' found, write everything
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
                            out.clear();
                            let raw_url_slice = &buf[url_start_idx..pos];
                            let (valid_url, suffix) = trim_url_end(raw_url_slice);

                            // Decode to the output buffer
                            if decode_chunk(valid_url, out, escape_space, verbose) {
                                has_changes = true;
                            }

                            // Write result + suffix (if any)
                            writer.write_all(out).context(WriteOutputSnafu)?;
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
                } // end while pos < len

                // Handle the edge case where the buffer is completely full of URL data.
                // We must process some of it to make room, but be careful not to split '%'
                // sequences.
                if offset == 0 && len == buf.len() {
                    if in_url {
                        // Safe cut point calculation:
                        // Don't cut if the end is '%', '%2', etc.
                        let mut cut_point = len;
                        if buf[len - 1] == b'%' {
                            cut_point = len - 1;
                        } else if len >= 2 && buf[len - 2] == b'%' {
                            cut_point = len - 2;
                        }

                        // If the whole buffer is just "%" or "%2", force move
                        if cut_point == 0 {
                            cut_point = len;
                        }

                        out.clear();
                        let chunk = &buf[..cut_point];
                        // Force decode chunk
                        if decode_chunk(chunk, out, escape_space, verbose) {
                            has_changes = true;
                        }
                        writer.write_all(out).context(WriteOutputSnafu)?;
                        total_processed += cut_point as u64;

                        // Set offset so `copy_within` at top of loop moves the remainder
                        offset = cut_point;
                        // url_start_idx logic: The next chunk continues the URL from index 0
                        url_start_idx = 0;
                    } else {
                        // Not in URL
                        writer.write_all(&buf[..len]).context(WriteOutputSnafu)?;
                        total_processed += len as u64;
                        len = 0;
                        offset = 0;
                    }
                }
            }

            Ok((total_processed, has_changes))
        })
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
