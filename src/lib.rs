#![warn(clippy::cargo)]

pub mod error;
mod log;
#[cfg(feature = "verbose-log")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    cell::UnsafeCell,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::Path,
};

pub use error::*;
use memchr::memchr;
use snafu::ResultExt;

use crate::log::{DecodeLogger, NoOpLogger, VerboseLogger};

const SMALL_FILE_THRESHOLD: u64 = 1024 * 1024;
const IO_BUF_SIZE: usize = 64 * 1024;

struct DecodeContext {
    io_buf: Vec<u8>,
    #[cfg(feature = "safe")]
    out_buf: Vec<u8>,
}

impl DecodeContext {
    fn new() -> Self {
        Self {
            io_buf: vec![0u8; IO_BUF_SIZE],
            #[cfg(feature = "safe")]
            out_buf: Vec::with_capacity(IO_BUF_SIZE * 2),
        }
    }
}

thread_local! {
    static DECODE_CTX: UnsafeCell<DecodeContext> = UnsafeCell::new(DecodeContext::new());
}

const URL_CHAR: [bool; 256] = gen_url_map(b"-+&@#/%?=~_|!:,.;");
#[cfg(feature = "safe")]
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

const HEX_INVALID: u8 = 0xFF;

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
fn check_url_prefix(slice: &[u8]) -> Option<usize> {
    if slice.len() >= 7 && slice.starts_with(b"http://") {
        Some(7)
    } else if slice.len() >= 8 && slice.starts_with(b"https://") {
        Some(8)
    } else {
        None
    }
}

#[cfg(feature = "safe")]
#[inline(always)]
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

// ============================================================================
// Core Logic
// ============================================================================

/// Direct decode
///
/// # Returns
///
/// changed or not
#[cfg(not(feature = "safe"))]
fn decode_chunk_to_writer<W: Write>(
    buf: &[u8],
    writer: &mut W,
    escape_space: bool,
    logger: &mut impl DecodeLogger,
) -> std::io::Result<bool> {
    let len = buf.len();
    let mut r = 0; // Read index
    let mut changed = false;

    logger.clear();

    while r < len {
        let remaining = &buf[r..];
        match memchr(b'%', remaining) {
            Some(pos) => {
                if pos > 0 {
                    writer.write_all(&remaining[..pos])?;

                    // Log plain text
                    logger.log_orig_slice(&remaining[..pos]);
                    logger.log_res_slice(&remaining[..pos]);
                    r += pos;
                }

                if r + 2 < len {
                    let h1 = buf[r + 1];
                    let h2 = buf[r + 2];
                    let v1 = HEX_MAP[h1 as usize];
                    let v2 = HEX_MAP[h2 as usize];

                    if (v1 | v2) != HEX_INVALID {
                        let decoded_byte = (v1 << 4) | v2;

                        if escape_space && decoded_byte == b' ' {
                            writer.write_all(b"%20")?;
                            logger.log_orig_slice(b"%20");
                            logger.log_res_slice(b"%20");
                        } else {
                            writer.write_all(&[decoded_byte])?;
                            changed = true;
                            logger.log_orig(b'%');
                            logger.log_orig(h1);
                            logger.log_orig(h2);
                            logger.log_res(decoded_byte);
                        }
                        r += 3;
                    } else {
                        writer.write_all(b"%")?;
                        logger.log_orig(b'%');
                        logger.log_res(b'%');
                        r += 1;
                    }
                } else {
                    writer.write_all(b"%")?;
                    logger.log_orig(b'%');
                    logger.log_res(b'%');
                    r += 1;
                }
            }
            None => {
                writer.write_all(remaining)?;
                logger.log_orig_slice(remaining);
                logger.log_res_slice(remaining);
                r = len;
            }
        }
    }
    logger.print_if_changed(changed);
    Ok(changed)
}

/// Decodes a URL slice, appends result to `out_vec`.
#[cfg(feature = "safe")]
fn decode_chunk_safe(
    url_bytes: &[u8],
    out_vec: &mut Vec<u8>,
    escape_space: bool,
    logger: &mut impl DecodeLogger,
) -> bool {
    let mut i = 0;
    let len = url_bytes.len();
    let mut changed = false;

    logger.clear();

    while i < len {
        let remaining = &url_bytes[i..];
        match memchr(b'%', remaining) {
            Some(pos) => {
                if pos > 0 {
                    let chunk = &remaining[..pos];
                    out_vec.extend_from_slice(chunk);
                    logger.log_orig_slice(chunk);
                    logger.log_res_slice(chunk);
                }
                i += pos;
                if i + 2 < len {
                    let h1 = url_bytes[i + 1];
                    let h2 = url_bytes[i + 2];
                    let v1 = HEX_MAP[h1 as usize];
                    let v2 = HEX_MAP[h2 as usize];

                    if (v1 | v2) != HEX_INVALID {
                        let decoded_byte = (v1 << 4) | v2;

                        if escape_space && decoded_byte == b' ' {
                            out_vec.extend_from_slice(b"%20");
                            logger.log_orig_slice(b"%20");
                            logger.log_res_slice(b"%20");
                        } else {
                            out_vec.push(decoded_byte);
                            changed = true;
                            logger.log_orig(b'%');
                            logger.log_orig(h1);
                            logger.log_orig(h2);
                            logger.log_res(decoded_byte);
                        }
                        i += 3;
                    } else {
                        out_vec.push(b'%');
                        logger.log_orig(b'%');
                        logger.log_res(b'%');
                        i += 1;
                    }
                } else {
                    out_vec.push(b'%');
                    logger.log_orig(b'%');
                    logger.log_res(b'%');

                    i += 1;
                }
            }
            None => {
                let chunk = &url_bytes[i..];
                out_vec.extend_from_slice(chunk);
                logger.log_orig_slice(chunk);
                logger.log_res_slice(chunk);

                i = len;
            }
        }
    }

    if simdutf8::basic::from_utf8(out_vec).is_err() {
        return false;
    }

    logger.print_if_changed(changed);
    changed
}

fn decode_stream_inner<R, W, L>(
    mut reader: R,
    writer: W,
    escape_space: bool,
    mut logger: L,
) -> Result<(u64, bool)>
where
    R: Read,
    W: Write,
    L: DecodeLogger,
{
    let mut writer = BufWriter::with_capacity(IO_BUF_SIZE, writer);

    DECODE_CTX.with(|ctx_ptr| {
        let ctx = unsafe { &mut *ctx_ptr.get() };

        let buf = ctx.io_buf.as_mut_slice();

        #[cfg(feature = "safe")]
        let out = &mut ctx.out_buf;

        let mut offset = 0;
        let mut len = 0;
        let mut total_processed = 0u64;
        let mut has_changes = false;

        let mut in_url = false;
        let mut url_start_idx: usize = 0;

        loop {
            if offset > 0 && len > offset {
                buf.copy_within(offset..len, 0);
                len -= offset;
                if in_url {
                    url_start_idx = url_start_idx.saturating_sub(offset);
                }
                offset = 0;
            } else if offset == len {
                len = 0;
                offset = 0;
            }

            let n = reader.read(&mut buf[len..]).context(ReadInputSnafu)?;
            if n == 0 {
                // EOF
                if len > 0 {
                    #[cfg(feature = "safe")]
                    out.clear();

                    if in_url {
                        #[cfg(feature = "safe")]
                        {
                            out.clear();
                            let (valid_url, suffix) = trim_url_end(&buf[url_start_idx..len]);
                            if decode_chunk_safe(valid_url, out, escape_space, &mut logger) {
                                has_changes = true;
                                writer.write_all(out).context(WriteOutputSnafu)?;
                                writer.write_all(suffix).context(WriteOutputSnafu)?;
                            } else {
                                writer
                                    .write_all(&buf[url_start_idx..len])
                                    .context(WriteOutputSnafu)?;
                            }
                        }

                        #[cfg(not(feature = "safe"))]
                        {
                            let chunk = &buf[url_start_idx..len];
                            let changed = decode_chunk_to_writer(
                                chunk,
                                &mut writer,
                                escape_space,
                                &mut logger,
                            )
                            .context(WriteOutputSnafu)?;
                            if changed {
                                has_changes = true;
                            }
                        }
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
                    match memchr(b'h', &buf[pos..len]) {
                        Some(rel_idx) => {
                            let h_idx = pos + rel_idx;
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
                        #[cfg(feature = "safe")]
                        {
                            out.clear();
                            let raw_url_slice = &buf[url_start_idx..pos];
                            let (valid_url, suffix) = trim_url_end(raw_url_slice);

                            if decode_chunk_safe(valid_url, out, escape_space, &mut logger) {
                                has_changes = true;
                                writer.write_all(out).context(WriteOutputSnafu)?;
                                writer.write_all(suffix).context(WriteOutputSnafu)?;
                            } else {
                                writer.write_all(raw_url_slice).context(WriteOutputSnafu)?;
                            }
                        }

                        #[cfg(not(feature = "safe"))]
                        {
                            let chunk = &buf[url_start_idx..pos];
                            let changed = decode_chunk_to_writer(
                                chunk,
                                &mut writer,
                                escape_space,
                                &mut logger,
                            )
                            .context(WriteOutputSnafu)?;

                            if changed {
                                has_changes = true;
                            }
                        }

                        total_processed += (pos - url_start_idx) as u64;
                        in_url = false;
                        offset = pos;
                    } else {
                        break;
                    }
                }
            }

            if offset == 0 && len == buf.len() {
                if in_url {
                    let mut cut_point = len;
                    if buf[len - 1] == b'%' {
                        cut_point = len - 1;
                    } else if len >= 2 && buf[len - 2] == b'%' {
                        cut_point = len - 2;
                    }
                    if cut_point == 0 {
                        cut_point = len;
                    }

                    let chunk = &mut buf[..cut_point];

                    #[cfg(not(feature = "safe"))]
                    {
                        let changed =
                            decode_chunk_to_writer(chunk, &mut writer, escape_space, &mut logger)
                                .context(WriteOutputSnafu)?;
                        if changed {
                            has_changes = true;
                        }
                    }

                    #[cfg(feature = "safe")]
                    {
                        out.clear();
                        // Using immutable borrow for safe logic
                        let chunk_imm = &chunk;
                        if decode_chunk_safe(chunk_imm, out, escape_space, &mut logger) {
                            has_changes = true;
                            writer.write_all(out).context(WriteOutputSnafu)?;
                        } else {
                            writer.write_all(chunk_imm).context(WriteOutputSnafu)?;
                        }
                    }

                    total_processed += cut_point as u64;
                    offset = cut_point;
                    url_start_idx = 0;
                } else {
                    writer.write_all(&buf[..len]).context(WriteOutputSnafu)?;
                    total_processed += len as u64;
                    len = 0;
                    offset = 0;
                }
            }
        }

        writer.flush().context(WriteOutputSnafu)?;
        Ok((total_processed, has_changes))
    })
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
    reader: R,
    writer: W,
    escape_space: bool,
    verbose: bool,
) -> Result<(u64, bool)>
where
    R: Read,
    W: Write,
{
    if verbose {
        let logger = VerboseLogger::new();
        decode_stream_inner(reader, writer, escape_space, logger)
    } else {
        let logger = NoOpLogger;
        decode_stream_inner(reader, writer, escape_space, logger)
    }
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
        let (_, changed) = decode_stream(input.as_bytes(), &mut buf, escape_space, verbose)?;
        changed
    };
    Ok((
        simdutf8::basic::from_utf8(&buf)
            .context(InvalidUtf8Snafu)?
            .to_owned(),
        changed,
    ))
}

/// Decodes a file and overwrites it if `dry_run` is false.
/// Note that for big files (> 1MB), this function will use a temporary file
/// to avoid too much memory allocation.
///
/// # Arguments
///
/// * `path` - The path to the file to decode.
/// * `escape_space` - Whether to decode `%20` to space.
/// * `dry_run` - Whether to print the result without overwriting the file.
/// * `verbose` - Whether to print verbose logs. (needs `verbose-log` feature)
/// * `p_counter` - The counter for processed files. (needs `verbose-log`
///   feature)
/// * `c_counter` - The counter for changed files. (needs `verbose-log` feature)
pub fn decode_file(
    path: &Path,
    escape_space: bool,
    dry_run: bool,
    #[cfg(feature = "verbose-log")] verbose: bool,
    #[cfg(feature = "verbose-log")] p_counter: &AtomicUsize,
    #[cfg(feature = "verbose-log")] c_counter: &AtomicUsize,
) -> Result<()> {
    #[cfg(not(feature = "verbose-log"))]
    let verbose = false;

    let file = File::open(path).context(OpenInputSnafu { path })?;
    let metadata = file.metadata().context(ReadInputSnafu)?;
    let file_len = metadata.len();
    let reader = BufReader::new(file);

    let (_processed_bytes, _changed) = if dry_run {
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

        // Ignore permission errors silently
        let _ = temp_file.as_file().set_permissions(metadata.permissions());

        let res = decode_stream(reader, &mut temp_file, escape_space, verbose)?;

        if res.1 {
            temp_file.persist(path).context(PersistTempSnafu { path })?;
        }
        res
    };

    #[cfg(feature = "verbose-log")]
    {
        p_counter.fetch_add(1, Ordering::Relaxed);
        if _changed {
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
