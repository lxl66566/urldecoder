use std::io::{self, Write as _};

pub trait DecodeLogger {
    fn new() -> Self
    where
        Self: Sized;
    fn log_orig(&mut self, byte: u8);
    fn log_orig_slice(&mut self, slice: &[u8]);
    fn log_res(&mut self, byte: u8);
    fn log_res_slice(&mut self, slice: &[u8]);
    fn print_if_changed(&mut self, changed: bool);
    fn clear(&mut self);
}

pub struct NoOpLogger;
impl DecodeLogger for NoOpLogger {
    #[inline(always)]
    fn new() -> Self {
        Self
    }
    #[inline(always)]
    fn log_orig(&mut self, _: u8) {}
    #[inline(always)]
    fn log_orig_slice(&mut self, _: &[u8]) {}
    #[inline(always)]
    fn log_res(&mut self, _: u8) {}
    #[inline(always)]
    fn log_res_slice(&mut self, _: &[u8]) {}
    #[inline(always)]
    fn print_if_changed(&mut self, _: bool) {}
    #[inline(always)]
    fn clear(&mut self) {}
}

const LOG_RES_CAPACITY: usize = 256;
const LOG_ORIG_CAPACITY: usize = LOG_RES_CAPACITY * 3;

const ELLIPSIS: &[u8; 3] = b"...";

pub struct VerboseLogger {
    res_len: usize,
    res_buf: [u8; LOG_RES_CAPACITY],
    orig_len: usize,
    orig_buf: [u8; LOG_ORIG_CAPACITY],
}

impl DecodeLogger for VerboseLogger {
    #[inline]
    fn new() -> Self {
        Self {
            res_len: 0,
            res_buf: [0; LOG_RES_CAPACITY],
            orig_len: 0,
            orig_buf: [0; LOG_ORIG_CAPACITY],
        }
    }

    #[inline(always)]
    fn log_orig(&mut self, byte: u8) {
        if self.orig_len < LOG_ORIG_CAPACITY {
            unsafe {
                *self.orig_buf.get_unchecked_mut(self.orig_len) = byte;
            }
            self.orig_len += 1;
        }
    }

    #[inline(always)]
    fn log_orig_slice(&mut self, slice: &[u8]) {
        unsafe {
            if self.orig_len + slice.len() < LOG_ORIG_CAPACITY {
                self.orig_buf
                    .get_unchecked_mut(self.orig_len..)
                    .copy_from_slice(slice);
                self.orig_len += slice.len();
            } else {
                let cp = LOG_ORIG_CAPACITY - self.orig_len;
                self.orig_buf
                    .get_unchecked_mut(self.orig_len..)
                    .copy_from_slice(&slice[..cp]);
                self.orig_len = LOG_ORIG_CAPACITY;
            }
        }
    }

    #[inline(always)]
    fn log_res(&mut self, byte: u8) {
        if self.res_len < LOG_RES_CAPACITY {
            unsafe {
                *self.res_buf.get_unchecked_mut(self.res_len) = byte;
            }
            self.res_len += 1;
        }
    }

    #[inline(always)]
    fn log_res_slice(&mut self, slice: &[u8]) {
        unsafe {
            if self.res_len + slice.len() < LOG_RES_CAPACITY {
                self.res_buf
                    .get_unchecked_mut(self.res_len..)
                    .copy_from_slice(slice);
                self.res_len += slice.len();
            } else {
                let cp = LOG_RES_CAPACITY - self.res_len;
                self.res_buf
                    .get_unchecked_mut(self.res_len..)
                    .copy_from_slice(&slice[..cp]);
                self.res_len = LOG_RES_CAPACITY;
            }
        }
    }

    #[inline]
    fn print_if_changed(&mut self, changed: bool) {
        if !changed {
            return;
        }

        self.print_impl();
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.res_len = 0;
        self.orig_len = 0;
    }
}

impl VerboseLogger {
    fn print_impl(&mut self) {
        let orig = &self.orig_buf[..self.orig_len];
        let res = &self.res_buf[..self.res_len];

        let stdout = io::stdout();
        let handle = stdout.lock();
        let mut writer = io::BufWriter::new(handle);
        writer.write_all("\x1b[31m- ".as_bytes()).unwrap();
        writer.write_all(orig).unwrap();
        if self.orig_len == LOG_ORIG_CAPACITY {
            writer.write_all(ELLIPSIS).unwrap();
        }
        writer.write_all("\x1b[0m\n\x1b[32m+ ".as_bytes()).unwrap();
        writer.write_all(res).unwrap();
        if self.res_len == LOG_RES_CAPACITY {
            writer.write_all(ELLIPSIS).unwrap();
        }
        writer.write_all("\x1b[0m\n".as_bytes()).unwrap();
        writer.flush().unwrap();
    }
}
