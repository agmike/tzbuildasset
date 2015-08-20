use std::error as error;
use std::io::{self, BufRead, Cursor, Write};
use std::fmt;
use std::path::{Path};
use std::process::{Command};
use std::result as result;
use std::str::{FromStr};

use regex::{Regex};

use displayprefix::{with_prefix};

lazy_static! {
    pub static ref TZUTIL_OUTPUT_MATCHER: Regex = {
        let r = Regex::new(r#"(?ix)
            (?P<prefix> [-+!;] ) \s*        # -, +, ! prefix
            < (?P<kuid> .*? ) > \s*        # asset kuid
            : \s* (?P<message> .+ ) \r?$   # optional windows eol
        "#);
        r.unwrap()
    };

    pub static ref TZUTL_RESULT_MATCHER: Regex = {
        let r = Regex::new(r#"(?ix)
            OK \s+
            \( \s* (?P<errors>   \d+) \s+ Errors   \s* ,
               \s* (?P<warnings> \d+) \s+ Warnings \s* \)
        "#);
        r.unwrap()
    };
}

pub type Result = result::Result<Output, Error>;

#[derive(Clone, Debug)]
pub struct Output {
    pub lines: Vec<String>,
    pub errors: u32,
    pub warnings: u32
}

impl Output {
    fn from_stdout(stdout: Vec<u8>) -> Self {
        let mut lines: Vec<String> = Cursor::new(stdout).lines().map(|l| l.unwrap()).collect();
        for line in &mut lines {
            if line.ends_with('\r') {
                line.pop().unwrap();
            }
        }

        let last_line = lines.pop().unwrap();
        let results = TZUTL_RESULT_MATCHER.captures(&last_line).unwrap();

        self::Output {
            lines: lines,
            errors: results.name("errors").and_then(|s| FromStr::from_str(s).ok()).unwrap(),
            warnings: results.name("warnings").and_then(|s| FromStr::from_str(s).ok()).unwrap(),
        }
    }
}

impl fmt::Display for self::Output {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for line in &self.lines {
            try!(write!(f, "{}\n", line));
        }
        try!(write!(f, "OK ({} Errors, {} Warnings)", self.errors, self.warnings));
        Ok(())
    }
}

#[derive(Debug)]
pub enum Error {
    Failure(self::Output),
    NotFound,
    Unknown(Box<error::Error>)
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Failure(_) => "TrainzUtil command failed",
            Error::NotFound => "TrainzUtil not found",
            Error::Unknown(_) => "unknown error",
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        match *self {
            Error::Unknown(ref e) => Some(&**e),
            _ => None
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Failure(ref output) =>
                write!(f, "TrainzUtil command failed with following output:\n{}", with_prefix(">", output)),
            Error::NotFound => write!(f, "TrainzUtil executable was not found"),
            Error::Unknown(ref e) => write!(f, "Unknown error: {}", e)
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => self::Error::NotFound,
            _ => self::Error::Unknown(Box::new(e))
        }
    }
}

pub fn execute(path: &Path, args: &[&str]) -> Result {
    let result = try!(Command::new(path).args(args).output());
    let output = self::Output::from_stdout(result.stdout);
    if result.status.success() {
        Ok(output)
    } else {
        Err(Error::Failure(output))
    }
}
