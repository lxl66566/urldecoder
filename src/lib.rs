pub mod error;
pub mod log;

#[cfg(feature = "verbose-log")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    fs,
    io::{self, BufWriter, Write},
    path::Path,
    sync::OnceLock,
};

pub use error::*;
use memchr::{memchr, memmem::Finder};
use snafu::ResultExt;
use tempfile::NamedTempFile;

#[cfg(feature = "verbose-log")]
use crate::log::VerboseLogger;
use crate::log::{DecodeLogger, NoOpLogger};

const SMALL_FILE_THRESHOLD: u64 = 256 * 1024;
const IO_BUF_SIZE: usize = 64 * 1024;
const URL_CHAR_BITMAP: [u32; 8] = gen_url_bitmap(b"-+&@#/%?=~_|!:,.;");
const URL_END_CHAR_BITMAP: [u32; 8] = gen_url_bitmap(b"-+&@#/%=~_|");

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

/// SWAR
#[inline(always)]
fn decode_hex_pair(h1: u8, h2: u8) -> u8 {
    let word = u16::from_le_bytes([h1, h2]);
    // '0'-'9' (0x30-0x39) -> 0-9
    // 'A'-'F' (0x41-0x46) -> 1-6
    // 'a'-'f' (0x61-0x66) -> 1-6
    let lower = word & 0x0F0F;
    // ('A'-'F' / 'a'-'f')
    let is_letter = (word & 0x4040) >> 6;
    // + 9 if is_letter
    let nibbles = lower + is_letter * 9;
    let decoded = ((nibbles & 0xFF) << 4) | (nibbles >> 8);
    decoded as u8
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

/// dispatch verbose to `decode_slice_to_writer` and `decode_in_place`
macro_rules! decode {
    ($func:ident($($args:expr),*), $verbose:expr) => {{
        if $verbose {
            #[cfg(feature = "verbose-log")]
            {
                let mut logger = VerboseLogger::new();
                $func($($args),*, &mut logger)
            }
            #[cfg(not(feature = "verbose-log"))]
            {
                $func($($args),*)
            }
        } else {
            #[cfg(feature = "verbose-log")]
            {
                let mut logger = NoOpLogger;
                $func($($args),*, &mut logger)
            }
            #[cfg(not(feature = "verbose-log"))]
            {
                $func($($args),*)
            }
        }
    }};
}

// region: in-place

/// Decode URL in-place using read and write pointers.
/// Returns the new length of the data.
pub fn decode_in_place(
    data: &mut [u8],
    escape_space: bool,
    #[cfg(feature = "verbose-log")] logger: &mut impl DecodeLogger,
) -> usize {
    if escape_space {
        decode_in_place_inner::<true>(
            data,
            #[cfg(feature = "verbose-log")]
            logger,
        )
    } else {
        decode_in_place_inner::<false>(
            data,
            #[cfg(feature = "verbose-log")]
            logger,
        )
    }
}

#[inline(always)]
fn decode_in_place_inner<const ESCAPE_SPACE: bool>(
    data: &mut [u8],
    #[cfg(feature = "verbose-log")] logger: &mut impl DecodeLogger,
) -> usize {
    let mut r = 0;
    let mut w = 0;
    let len = data.len();
    let finder = http_finder();

    while r < len {
        if let Some(match_idx) = finder.find(&data[r..]) {
            let start = r + match_idx;

            let is_http = data[start..].starts_with(b"http://");
            let is_https = data[start..].starts_with(b"https://");

            if is_http || is_https {
                // Copy plain text before URL
                if start > r {
                    let copy_len = start - r;
                    if w != r {
                        data.copy_within(r..start, w);
                    }
                    w += copy_len;
                }

                // Find URL end
                let prefix_len = if is_https { 8 } else { 7 };
                let mut end = start + prefix_len;
                while end < len && is_url_char(data[end]) {
                    end += 1;
                }

                let mut valid_end = end;
                while valid_end > start {
                    if is_url_end_char(unsafe { *data.get_unchecked(valid_end - 1) }) {
                        break;
                    }
                    valid_end -= 1;
                }

                // Decode URL in-place
                w = decode_url_in_place_indices::<ESCAPE_SPACE>(
                    data,
                    start,
                    valid_end,
                    w,
                    #[cfg(feature = "verbose-log")]
                    logger,
                );

                // Copy suffix after trimmed punctuation
                let suffix_len = end - valid_end;
                if suffix_len > 0 {
                    if w != valid_end {
                        data.copy_within(valid_end..end, w);
                    }
                    w += suffix_len;
                }

                r = end;
            } else {
                // Found `http` but not a url
                let copy_len = start + 4 - r;
                if w != r {
                    data.copy_within(r..start + 4, w);
                }
                w += copy_len;
                r = start + 4;
            }
        } else {
            // Copy remaining
            if r < len {
                let copy_len = len - r;
                if w != r {
                    data.copy_within(r..len, w);
                }
                w += copy_len;
            }
            break;
        }
    }
    w
}

#[inline(always)]
fn decode_url_in_place_indices<const ESCAPE_SPACE: bool>(
    data: &mut [u8],
    src_start: usize,
    src_end: usize,
    mut dst: usize,
    #[cfg(feature = "verbose-log")] logger: &mut impl DecodeLogger,
) -> usize {
    #[cfg(not(feature = "verbose-log"))]
    let mut logger = NoOpLogger;
    logger.clear();

    let mut i = src_start;
    let first_pct = match memchr(b'%', &data[i..src_end]) {
        Some(idx) => idx,
        None => {
            let len = src_end - i;
            logger.log_orig_slice(&data[i..src_end]);
            logger.log_res_slice(&data[i..src_end]);
            if dst != i {
                data.copy_within(i..src_end, dst);
            }
            return dst + len;
        }
    };

    if first_pct > 0 {
        logger.log_orig_slice(&data[i..i + first_pct]);
        logger.log_res_slice(&data[i..i + first_pct]);
        if dst != i {
            data.copy_within(i..i + first_pct, dst);
        }
        dst += first_pct;
        i += first_pct;
    }

    let mut literal_start = i;
    let mut changed = false;

    while i < src_end {
        if data[i] == b'%' && i + 2 < src_end {
            let h1 = data[i + 1];
            let h2 = data[i + 2];
            let decoded = decode_hex_pair(h1, h2);
            if ESCAPE_SPACE && decoded == b' ' {
                i += 3;
                continue;
            }

            changed = true;
            if i > literal_start {
                let len = i - literal_start;
                logger.log_orig_slice(&data[literal_start..i]);
                logger.log_res_slice(&data[literal_start..i]);
                if dst != literal_start {
                    data.copy_within(literal_start..i, dst);
                }
                dst += len;
            }

            logger.log_orig(b'%');
            logger.log_orig(h1);
            logger.log_orig(h2);
            logger.log_res(decoded);

            data[dst] = decoded;
            dst += 1;
            i += 3;
            literal_start = i;
            continue;
        }
        if data[i] == b'%' {
            i += 1;
        } else {
            match memchr(b'%', &data[i..src_end]) {
                Some(offset) => i += offset,
                None => i = src_end,
            }
        }
    }

    if literal_start < src_end {
        let len = src_end - literal_start;
        logger.log_orig_slice(&data[literal_start..src_end]);
        logger.log_res_slice(&data[literal_start..src_end]);
        if dst != literal_start {
            data.copy_within(literal_start..src_end, dst);
        }
        dst += len;
    }

    logger.print_if_changed(changed);
    dst
}

#[cfg(not(feature = "safe"))]
fn decode_file_in_place(
    path: &Path,
    escape_space: bool,
    #[allow(unused)] verbose: bool,
    #[cfg(feature = "verbose-log")] p_counter: &AtomicUsize,
    #[cfg(feature = "verbose-log")] c_counter: &AtomicUsize,
) -> Result<()> {
    use std::fs::{self, OpenOptions};

    let metadata = fs::metadata(path).context(ReadInputSnafu)?;
    let file_len = metadata.len();

    if file_len == 0 {
        #[cfg(feature = "verbose-log")]
        p_counter.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    #[allow(unused)]
    let changed = if file_len < SMALL_FILE_THRESHOLD {
        let mut buf = fs::read(path).context(ReadInputSnafu)?;
        let new_len = decode!(decode_in_place(&mut buf, escape_space), verbose);
        let is_changed = new_len < file_len as usize;

        if is_changed {
            fs::write(path, &buf[..new_len]).context(WriteOutputSnafu)?;
        }
        is_changed
    } else {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .context(OpenInputSnafu { path })?;

        let mut mmap = unsafe {
            memmap2::MmapOptions::new()
                .map_mut(&file)
                .context(ReadInputSnafu)?
        };

        let new_len = decode!(decode_in_place(&mut mmap, escape_space), verbose);
        let is_changed = new_len < file_len as usize;

        if is_changed {
            mmap.flush().context(WriteOutputSnafu)?;
            drop(mmap);
            file.set_len(new_len as u64).context(WriteOutputSnafu)?;
        }
        is_changed
    };

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

// region: to writer

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
    // static dispatch: completely remove `escape_space` branch at compile time
    if escape_space {
        decode_inner::<true, W>(
            url,
            writer,
            #[cfg(feature = "verbose-log")]
            logger,
        )
    } else {
        decode_inner::<false, W>(
            url,
            writer,
            #[cfg(feature = "verbose-log")]
            logger,
        )
    }
}

#[inline(always)]
fn decode_inner<const ESCAPE_SPACE: bool, W: Write>(
    url: &[u8],
    writer: &mut W,
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
    let mut literal_start = i; // for batch write

    while i < len {
        if url[i] == b'%' && i + 2 < len {
            let h1 = url[i + 1];
            let h2 = url[i + 2];
            let decoded = decode_hex_pair(h1, h2);
            if ESCAPE_SPACE && decoded == b' ' {
                i += 3;
                continue;
            }

            changed = true;
            if i > literal_start {
                writer.write_all(&url[literal_start..i])?;
                logger.log_orig_slice(&url[literal_start..i]);
                logger.log_res_slice(&url[literal_start..i]);
            }
            writer.write_all(&[decoded])?;
            logger.log_orig(b'%');
            logger.log_orig(h1);
            logger.log_orig(h2);
            logger.log_res(decoded);

            i += 3;
            literal_start = i;
            continue;
        }
        if url[i] == b'%' {
            i += 1;
        } else {
            match memchr(b'%', &url[i..]) {
                Some(offset) => i += offset,
                None => i = len,
            }
        }
    }
    if literal_start < len {
        writer.write_all(&url[literal_start..len])?;
        logger.log_orig_slice(&url[literal_start..len]);
        logger.log_res_slice(&url[literal_start..len]);
    }

    logger.print_if_changed(changed);
    Ok(changed)
}

/// Decode String
pub fn decode_str(
    input: &str,
    escape_space: bool,
    #[cfg(feature = "verbose-log")] verbose: bool,
) -> Result<(String, bool)> {
    #[cfg(not(feature = "verbose-log"))]
    let verbose = false;

    let mut buf = input.as_bytes().to_vec();

    let new_len = decode!(decode_in_place(&mut buf, escape_space), verbose);

    let changed = new_len < buf.len();
    buf.truncate(new_len);

    Ok((
        simdutf8::basic::from_utf8(&buf)
            .context(InvalidUtf8Snafu)?
            .to_owned(),
        changed,
    ))
}

/// Decode file and overwrite.
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

    #[cfg(not(feature = "safe"))]
    {
        if !dry_run {
            return decode_file_in_place(
                path,
                escape_space,
                verbose,
                #[cfg(feature = "verbose-log")]
                p_counter,
                #[cfg(feature = "verbose-log")]
                c_counter,
            );
        }
    }

    // Safe fallback / dry_run logic
    let metadata = fs::metadata(path).context(ReadInputSnafu)?;
    let file_len = metadata.len();

    if file_len == 0 {
        #[cfg(feature = "verbose-log")]
        p_counter.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    #[allow(unused)]
    let mut changed = false;

    #[allow(unused)]
    if file_len < SMALL_FILE_THRESHOLD {
        let mut buf = fs::read(path).context(ReadInputSnafu)?;
        let new_len = decode!(decode_in_place(&mut buf, escape_space), verbose);
        changed = new_len < buf.len();

        if changed && !dry_run {
            buf.truncate(new_len);
            let parent = path.parent().unwrap_or_else(|| Path::new("."));

            let mut temp_file =
                NamedTempFile::new_in(parent).context(CreateTempSnafu { dir: parent })?;

            temp_file.write_all(&buf).context(WriteOutputSnafu)?;
            temp_file.flush().context(WriteOutputSnafu)?;

            // Set permissions
            let _ = temp_file.as_file().set_permissions(metadata.permissions());
            temp_file.persist(path).context(PersistTempSnafu { path })?;
        }
    } else {
        // mmap
        let file = fs::File::open(path).context(OpenInputSnafu { path })?;
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                .map(&file)
                .context(ReadInputSnafu)?
        };

        if dry_run {
            let mut sink = io::sink();
            changed = decode!(
                decode_slice_to_writer(&mmap, &mut sink, escape_space),
                verbose
            )
            .context(WriteOutputSnafu)?;
        } else {
            let parent = path.parent().unwrap_or_else(|| Path::new("."));

            let mut temp_file =
                NamedTempFile::new_in(parent).context(CreateTempSnafu { dir: parent })?;

            {
                let mut buf_writer = BufWriter::with_capacity(IO_BUF_SIZE, &mut temp_file);
                changed = decode!(
                    decode_slice_to_writer(&mmap, &mut buf_writer, escape_space),
                    verbose
                )
                .context(WriteOutputSnafu)?;
                buf_writer.flush().context(WriteOutputSnafu)?;
            }

            drop(mmap);
            drop(file);

            if changed {
                // Set permissions AFTER writing to avoid PermissionDenied if original is
                // read-only
                let _ = temp_file.as_file().set_permissions(metadata.permissions());
                temp_file.persist(path).context(PersistTempSnafu { path })?;
            }
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
    fn test_decode_hex_pair() {
        let hex_chars: Vec<u8> = b"0123456789ABCDEFabcdef".to_vec();
        for &c1 in &hex_chars {
            for &c2 in &hex_chars {
                let tmp = [c1, c2];
                let s = std::str::from_utf8(&tmp).unwrap();
                let expected = u8::from_str_radix(s, 16).unwrap();
                let actual = decode_hex_pair(c1, c2);
                assert_eq!(actual, expected);
            }
        }
    }

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
        let t1 = temp.into_temp_path();
        let test_str = "xxxxhttps://www.baidu.com/s?ie=UTF-8&wd=%E5%A4%A9%E6%B0%94xxxx";
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
            "xxxxhttps://www.baidu.com/s?ie=UTF-8&wd=天气xxxx"
        );
    }
}
