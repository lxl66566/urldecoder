pub trait DecodeLogger {
    fn init(enabled: bool) -> Self;
    fn log_orig(&self, byte: u8);
    fn log_res(&self, byte: u8);
    fn print_if_changed(&self, changed: bool);
}

#[cfg(not(feature = "verbose-log"))]
pub mod logger {
    use super::DecodeLogger;

    pub struct Logger;
    impl DecodeLogger for Logger {
        #[inline(always)]
        fn init(_: bool) -> Self {
            Self {}
        }
        #[inline(always)]
        fn log_orig(&self, _: u8) {}
        #[inline(always)]
        fn log_res(&self, _: u8) {}
        #[inline(always)]
        fn print_if_changed(&self, _: bool) {}
    }
}

#[cfg(feature = "verbose-log")]
pub mod logger {
    use std::cell::RefCell;

    #[cfg(feature = "color")]
    use colored::Colorize;

    use super::DecodeLogger;

    const LOG_RES_CAPACITY: usize = 256;
    const LOG_ORIG_CAPACITY: usize = LOG_RES_CAPACITY * 3;

    thread_local! {
        static LOG_RES_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(LOG_RES_CAPACITY));
        static LOG_ORIG_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(LOG_ORIG_CAPACITY));
    }

    #[inline(always)]
    fn push_limit(vec: &mut Vec<u8>, byte: u8, limit: usize) {
        if vec.len() < limit {
            vec.push(byte);
        }
    }

    pub struct Logger {
        enabled: bool,
    }

    impl DecodeLogger for Logger {
        #[inline]
        fn init(enabled: bool) -> Self {
            if enabled {
                LOG_RES_BUF.with(|b| b.borrow_mut().clear());
                LOG_ORIG_BUF.with(|b| b.borrow_mut().clear());
            }
            Self { enabled }
        }

        #[inline]
        fn log_orig(&self, byte: u8) {
            if self.enabled {
                LOG_ORIG_BUF.with(|buf| push_limit(&mut buf.borrow_mut(), byte, LOG_ORIG_CAPACITY));
            }
        }

        #[inline]
        fn log_res(&self, byte: u8) {
            if self.enabled {
                LOG_RES_BUF.with(|buf| push_limit(&mut buf.borrow_mut(), byte, LOG_RES_CAPACITY));
            }
        }

        #[inline]
        fn print_if_changed(&self, changed: bool) {
            if self.enabled && changed {
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
        }
    }
}
