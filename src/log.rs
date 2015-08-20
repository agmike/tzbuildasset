use std::fmt;
use std::sync::{Mutex};

lazy_static! {
    static ref LOGGER: Mutex<Option<Logger>> = Mutex::new(None);
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Mode { Silent, Normal, Verbose }

impl Mode {
    fn accepts(self, other: Mode) -> bool {
        match self {
            Mode::Silent => other == Mode::Silent,
            Mode::Normal => other == Mode::Normal,
            Mode::Verbose => other == Mode::Normal || other == Mode::Verbose,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Severity { Error, Warn, Info }

#[derive(Copy, Clone, Default, Debug)]
pub struct Statistics {
    pub errors: u32,
    pub warnings: u32,
}

struct Logger {
    pub mode: Mode,
    pub stats: Statistics,
}

pub fn init(mode: Mode) {
    *LOGGER.lock().unwrap() = Some(Logger {
        mode: mode,
        stats: Statistics::default(),
    });
}

pub fn log(mode: Mode, severity: Severity, args: fmt::Arguments) {
    if let Some(ref mut logger) = *LOGGER.lock().unwrap() {
        match severity {
            Severity::Error => logger.stats.errors += 1,
            Severity::Warn => logger.stats.warnings += 1,
            _ => ()
        }

        if logger.mode.accepts(mode) {
            if logger.mode == Mode::Silent {
                print!("{}\n", args);
            } else {
                match severity {
                    Severity::Error => print!("ERROR {}\n", args),
                    Severity::Warn  => print!("WARN  {}\n", args),
                    Severity::Info  => print!("INFO  {}\n", args),
                }
            }
        }
    }
}

macro_rules! log_silent {
    ($severity:ident, $($args:tt)*) => ({
        ::log::log(::log::Mode::Silent, ::log::Severity::$severity, format_args!($($args)*))
    })
}

macro_rules! log_normal {
    ($severity:ident, $($args:tt)*) => ({
        ::log::log(::log::Mode::Normal, ::log::Severity::$severity, format_args!($($args)*))
    })
}

macro_rules! log_verbose {
    ($severity:ident, $($args:tt)*) => ({
        ::log::log(::log::Mode::Verbose, ::log::Severity::$severity, format_args!($($args)*))
    })
}

pub fn statistics() -> Statistics {
    let logger: &Option<Logger> = &*LOGGER.lock().unwrap();
    logger.as_ref().unwrap().stats.clone()
}
