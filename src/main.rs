extern crate docopt;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate regex;
extern crate rustc_serialize;
extern crate tempdir;

use std::env;
use std::error::{self, Error};
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::str::{FromStr};
use std::sync::{Arc, Mutex};

use docopt::{Docopt};

use log::{LogLevel, LogLevelFilter, LogMetadata, LogRecord};

use regex::{Regex};

use tempdir::TempDir;

const USAGE: &'static str = r#"
Trainz Asset Builder.

Builds all assets within given path:
1. Copies asset to temporary directory.
2. Replaces asset KUID with a dummy one.
3. Installs asset to Trainz.
4. Commits and validates it.
5. Removes it from Trainz.

Assets are determined by recursively searching in directories for `config.txt` file which contains
string like:
kuid <(kuid|kuid2):[0-9]+:[0-9]+:[0-9]+>

Directory containing such file is treated as asset root and has steps 1-4 performed on it.

Usage:
  tzbuildasset build <path> [--trainzutil=PATH] [(-v | --verbose)]
  tzbuildasset (-h | --help)
  tzbuildasset --version

Options:
  -v --verbose         Detailed output
  --trainzutil=PATH    Path to TrainzUtil executable [default: TrainzUtil]
  -h --help            Show this help text
  --version            Show version
"#;

#[derive(Debug, RustcDecodable)]
struct Args {
    flag_trainzutil: String,
    flag_verbose: bool,
    flag_version: bool,
    arg_path: String,
    cmd_build: bool,
}


fn main() {
    let args: Args = Docopt::new(USAGE)
                            .and_then(|d| d.decode())
                            .unwrap_or_else(|e| e.exit());
    if args.flag_version {
        println!("{}", env!("CARGO_PKG_VERSION"));
    }
    else if args.cmd_build {
        build(args);
    }
}


lazy_static! {
    static ref KUID_MATCHER: Regex = {
        let r = Regex::new(r#"(?ixm)^kuid \s+ <(?P<kuid> kuid2? : \d+ : \d+ ( : \d+ )? )>"#);
        r.unwrap()
    };
}

const KUID_DUMMY: &'static str = "kuid:298469:999999";
const KUID_DUMMY_TAG: &'static str = "kuid  <kuid:298469:999999>";


fn build(args: Args) {
    let stats = OutputLogger::init(args.flag_verbose);

    let build_path = Path::new(&args.arg_path);
    let trainzutil_path = Path::new(&args.flag_trainzutil);

    trace!("Build path: {}", build_path.display());
    trace!("TrainzUtil path: {}", trainzutil_path.display());

    match execute_trainzutil(trainzutil_path, &["version"]) {
        Ok(output) => trace!("TrainzUtil version: {}", output.lines[0]),
        Err(e) => {
            error!("TrainzUtil error: {}", e);
            process::exit(2);
        }
    }

    for asset in locate_assets(build_path) {
        build_asset(asset, build_path, trainzutil_path);
    }

    let stats = stats.lock().unwrap();
    process::exit(if stats.error_count > 0 { 1 } else { 0 });
}


#[derive(Debug)]
struct Asset {
    pub kuid: String,
    pub path: PathBuf
}

fn locate_assets(build_path: &Path) -> Vec<Asset> {
    let mut located_assets = Vec::new();
    locate_assets_recursive(build_path, &mut located_assets);
    located_assets
}

const KNOWN_DIRS: &'static [&'static str] = &[".git", ".hg"];

fn locate_assets_recursive(path: &Path, located_assets: &mut Vec<Asset>) {

    trace!("Entering directory: {0}", path.display());

    // First try to read config.txt file
    let config_path = path.join("config.txt");
    if let Ok(config_file) = File::open(&config_path) {
        // config.txt exists: read contents and try to find kuid
        trace!("Found config.txt: {}", config_path.display());
        let mut contents = String::new();
        BufReader::new(config_file).read_to_string(&mut contents).unwrap();

        if let Some(caps) = KUID_MATCHER.captures(&contents) {
            // Found kuid - adding this path as asset root and exiting
            let kuid = caps.name("kuid").unwrap();
            trace!("Found kuid: {}", kuid);
            info!("Found asset: <{kuid}>, {path}", kuid = kuid, path = path.display());

            located_assets.push(Asset {
                kuid: kuid.to_owned(),
                path: path.to_owned()
            });
            return;
        }
    }
    // otherwise recursively walk all directories
    else {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.metadata().unwrap().is_dir() {
                // Skip known directory names
                if entry.file_name().to_str().map(|s| KNOWN_DIRS.contains(&s)).unwrap_or(false) {
                    continue;
                }
                locate_assets_recursive(&entry.path(), located_assets);
            }
        }
    }
}


fn build_asset(asset: Asset, build_path: &Path, trainzutil_path: &Path) {
    info!("Building asset <{}>", &asset.kuid);

    trace!("Creating temporary directory...");
    let temp_dir = TempDir::new("tzassetbuild").unwrap();
    trace!("Path: {}", temp_dir.path().display());
    copy_dir(&asset.path, temp_dir.path()).unwrap();

    trace!("Replacing kuid...");
    replace_kuid(temp_dir.path());

    trace!("Installing...");
    let output = match execute_trainzutil(trainzutil_path,
            &["installfrompath", temp_dir.path().to_str().unwrap()]) {
        Ok(output) => output,
        Err(e) => {
            error!("Failed to install asset <{}>: {}", &asset.kuid, e);
            return;
        }
    };
    trace!("Success! TrainzUtil output:\n{}", with_prefix(">", &output));

    trace!("Committing...");
    let output = match execute_trainzutil(trainzutil_path, &["commit", KUID_DUMMY]) {
        Ok(output) => output,
        Err(e) => {
            error!("Failed to commit asset <{}>: {}", &asset.kuid, e);
            return;
        }
    };
    trace!("Success! TrainzUtil output:\n{}", with_prefix(">", &output));

    std::thread::sleep_ms(2_000);

    trace!("Validating...");
    let output = match execute_trainzutil(trainzutil_path, &["validate", KUID_DUMMY]) {
        Ok(output) => output,
        Err(e) => {
            error!("Failed to validate asset <{}>: {}", &asset.kuid, e);
            return;
        }
    };
    trace!("Success! TrainzUtil output:\n{}", with_prefix(">", &output));

    trace!("Deleting...");
    let output = match execute_trainzutil(trainzutil_path, &["delete", KUID_DUMMY]) {
        Ok(output) => output,
        Err(e) => {
            error!("Failed to delete asset <{}>: {}", &asset.kuid, e);
            return;
        }
    };
    trace!("Success! TrainzUtil output:\n{}", with_prefix(">", &output));
}

fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    trace!("Copying directory {} to {}", src.display(), dst.display());
    for entry in try!(fs::read_dir(src)) {
        let entry = try!(entry);
        let entry_src_path = &entry.path();
        let entry_dst_path = &{
            let mut buf = PathBuf::from(dst);
            buf.push(entry.file_name());
            buf
        };

        if try!(entry.file_type()).is_file() {
            trace!("Copying file {} to {}", entry_src_path.display(), entry_dst_path.display());
            try!(fs::copy(entry_src_path, entry_dst_path));
        } else if try!(entry.file_type()).is_dir() {
            try!(fs::create_dir(entry_dst_path));
            try!(copy_dir(entry_src_path, entry_dst_path));
        }
    }
    Ok(())
}

fn replace_kuid(asset_root: &Path) -> io::Result<()> {
    let mut config_path = PathBuf::from(asset_root);
    config_path.push("config.txt");

    let mut text = {
        let mut text = String::new();
        let mut file = try!(File::open(&config_path));
        try!(file.read_to_string(&mut text));
        text
    };

    text = KUID_MATCHER.replace(&text, KUID_DUMMY_TAG);

    let mut file = try!(File::create(&config_path));
    try!(file.write_all(text.as_bytes()));
    Ok(())
}


//
// TrainzUtil handling


type TrainUtilResult = Result<TrainzUtilOutput, TrainzUtilError>;

#[derive(Clone, Debug)]
struct TrainzUtilOutput {
    pub lines: Vec<String>,
    pub errors: u32,
    pub warnings: u32
}

impl TrainzUtilOutput {
    fn from_stdout(stdout: Vec<u8>) -> Self {
        let mut lines: Vec<String> = Cursor::new(stdout).lines().map(|l| l.unwrap()).collect();
        for line in &mut lines {
            if line.ends_with('\r') {
                line.pop().unwrap();
            }
        }

        let last_line = lines.pop().unwrap();
        let results = Regex::new(r#"(?ix)
            OK \s+
            \( \s* (?P<errors>   \d+) \s+ Errors   \s* ,
               \s* (?P<warnings> \d+) \s+ Warnings \s* \)
        "#).unwrap().captures(&last_line).unwrap();

        TrainzUtilOutput {
            lines: lines,
            errors: results.name("errors").and_then(|s| FromStr::from_str(s).ok()).unwrap(),
            warnings: results.name("warnings").and_then(|s| FromStr::from_str(s).ok()).unwrap(),
        }
    }
}

impl fmt::Display for TrainzUtilOutput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for line in &self.lines {
            try!(write!(f, "{}\n", line));
        }
        try!(write!(f, "OK ({} Errors, {} Warnings)", self.errors, self.warnings));
        Ok(())
    }
}

struct DisplayPrefix<'a, T: fmt::Display> {
    prefix: &'a str,
    data: T
}

impl<'a, T: fmt::Display> fmt::Display for DisplayPrefix<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let text = format!("{}", self.data);
        let mut first_line = true;
        for line in text.lines() {
            if !first_line {
                try!(f.write_str("\n"));
            }
            first_line = false;
            try!(write!(f, "{}{}", self.prefix, line));
        }
        Ok(())
    }
}

fn with_prefix<'a, T>(prefix: &'a str, data: T) -> DisplayPrefix<'a, T>
        where T: fmt::Display {
    DisplayPrefix {
        prefix: prefix,
        data: data
    }
}

#[derive(Debug)]
enum TrainzUtilError {
    Failure(TrainzUtilOutput),
    NotFound,
    Unknown(Box<Error>)
}

impl Error for TrainzUtilError {
    fn description(&self) -> &str {
        match *self {
            TrainzUtilError::Failure(_) => "TrainzUtil command failed",
            TrainzUtilError::NotFound => "TrainzUtil not found",
            TrainzUtilError::Unknown(_) => "unknown error",
        }
    }

    fn cause(&self) -> Option<&Error> {
        match *self {
            TrainzUtilError::Unknown(ref e) => Some(&**e),
            _ => None
        }
    }
}

impl fmt::Display for TrainzUtilError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TrainzUtilError::Failure(ref output) =>
                write!(f, "TrainzUtil command failed with following output:\n{}", with_prefix(">", output)),
            TrainzUtilError::NotFound => write!(f, "TrainzUtil executable was not found"),
            TrainzUtilError::Unknown(ref e) => write!(f, "Unknown error: {}", e)
        }
    }
}

impl From<io::Error> for TrainzUtilError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => TrainzUtilError::NotFound,
            _ => TrainzUtilError::Unknown(Box::new(e))
        }
    }
}

fn execute_trainzutil(path: &Path, args: &[&str]) -> TrainUtilResult {
    let result = try!(Command::new(path).args(args).output());
    let output = TrainzUtilOutput::from_stdout(result.stdout);
    if result.status.success() {
        Ok(output)
    } else {
        Err(TrainzUtilError::Failure(output))
    }
}


//
// Logging


#[derive(Copy, Clone, Debug)]
struct Statistics {
    pub error_count: u32
}

impl Statistics {
    fn new() -> Self { Statistics { error_count: 0 } }
}


struct OutputLogger {
    verbose: bool,
    stats: Arc<Mutex<Statistics>>
}

impl OutputLogger {
    fn init(verbose: bool) -> Arc<Mutex<Statistics>> {
        let stats = Arc::new(Mutex::new(Statistics::new()));
        let logger_stats = stats.clone();

        log::set_logger(move |max_log_level| {
            max_log_level.set(if verbose { LogLevelFilter::Trace } else { LogLevelFilter::Info });

            Box::new(OutputLogger {
                verbose: verbose,
                stats: logger_stats
            })
        });

        stats
    }
}

impl log::Log for OutputLogger {
    fn enabled(&self, metadata: &log::LogMetadata) -> bool {
        metadata.level() <= (if self.verbose { LogLevel::Trace } else { LogLevel::Info })
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            if record.level() <= LogLevel::Error {
                println!("ERROR {}", record.args());
                    self.stats.lock().unwrap().error_count += 1;
            } else {
                println!("{}", record.args());
            };
        }
    }
}
