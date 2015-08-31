extern crate docopt;
#[macro_use] extern crate lazy_static;
extern crate regex;
extern crate rustc_serialize;

use std::env;
use std::error::{Error};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::iter;
use std::ffi::{OsString};
use std::fs::{self, File};
use std::path::{self, Path, PathBuf};
use std::process::{self};

use docopt::{Docopt};

use regex::{Regex};

use displayprefix::{with_prefix};


mod displayprefix;
#[macro_use] mod log;
mod trainzutil;


lazy_static! {
    static ref KUID_MATCHER: Regex = {
        let r = Regex::new(r#"(?ixm)^kuid \s+ <(?P<kuid> kuid2? : \d+ : \d+ ( : \d+ )? )>"#);
        r.unwrap()
    };

    static ref USERNAME_MATCHER: Regex = {
        let r = Regex::new(r#"(?ixm)^username \s+ " (?P<name> [^"]* ) "#);
        r.unwrap()
    };
}


const USAGE: &'static str = r#"
Trainz Asset Builder.

Usage:
  tzbuildasset [options] [INPUT]

Options:
  -r --recursive       Search for assets in all subfolders recursively
  -c --config          Show path to config.txt in output
  -k --kuid            Show KUID in output
  --trainzutil PATH    Path to TrainzUtil executable
  -v --verbose         Detailed output
  -s --silent          Silent output
  --temp-dir PATH      Use specified temporary directory
  -h --help            Show help
  --version            Show version

Installs and validates all assets within given path with TrainzUtil.

Assets are determined by searching for `config.txt` file which contains string like:
kuid <(kuid|kuid2):[0-9]+:[0-9]+:[0-9]+>
"#;

#[derive(Debug, RustcDecodable)]
#[allow(non_snake_case)]
struct Args {
    flag_recursive: bool,
    flag_config: bool,
    flag_kuid: bool,
    flag_trainzutil: Option<String>,
    flag_verbose: bool,
    flag_silent: bool,
    flag_temp_dir: Option<String>,
    arg_INPUT: Option<String>,

    flag_help: bool,
    flag_version: bool,
}


fn main() {
    let args: Args = Docopt::new(USAGE)
                            .and_then(|d| d.decode())
                            .unwrap_or_else(|e| e.exit());
    let log_mode = match (args.flag_silent, args.flag_verbose) {
        (true, _) => log::Mode::Silent,
        (_, true) => log::Mode::Verbose,
        (_, _) => log::Mode::Normal,
    };
    log::init(log_mode);

    if args.flag_version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    else {
        let success = build(&BuildArguments {
            build_path: &env::current_dir().unwrap().join(args.arg_INPUT.unwrap_or(String::new())),
            trainzutil_path: Path::new(&args.flag_trainzutil.map(|s| OsString::from(s))
                    .or_else(|| env::var_os("TRAINZUTIL_PATH"))
                    .unwrap_or_else(|| OsString::from("TrainzUtil"))),
            temp_path: args.flag_temp_dir.as_ref().map(|s| Path::new(s)),
            show_config_path: args.flag_config,
            show_kuid: args.flag_kuid,
            recursive: args.flag_recursive
        });

        process::exit(if success { 0 } else { 1 });
    }
}


struct BuildArguments<'a> {
    pub build_path: &'a Path,
    pub trainzutil_path: &'a Path,
    pub temp_path: Option<&'a Path>,
    pub show_config_path: bool,
    pub show_kuid: bool,
    pub recursive: bool
}


fn build(args: &BuildArguments) -> bool {

    log_verbose!(Info, "Build path: {}", args.build_path.display());
    log_verbose!(Info, "TrainzUtil path: {}", args.trainzutil_path.display());

    match trainzutil::execute(args.trainzutil_path, &["version"]) {
        Ok(output) => log_verbose!(Info, "TrainzUtil version: {}", output.lines[0]),
        Err(e) => {
            log_normal!(Error, "Unable to execute TrainzUtil: {}", e);
            log_silent!(Error, "- <NULL> : TrainzUtil not found");
            return false;
        }
    }

    let assets = locate_assets(args.build_path, args.recursive);
    let mut installed = Vec::with_capacity(assets.len());
    let mut succeeded_count = 0usize;

    for asset in &assets {
        if install_asset(asset, args) {
            installed.push(asset);
        }
    }

    for asset in &installed {
        if validate_asset(asset, args) {
            succeeded_count += 1;
        }
    }

    let output = format!("BUILD {} ({} Total, {} Succeeded, {} Failed)",
            if succeeded_count == assets.len() { "SUCCESS" } else { "FAILED " },
            assets.len(),
            succeeded_count,
            assets.len() - succeeded_count);
    let line: String = iter::repeat('=').take(output.len()).collect();

    log_normal!(Info, "{}", line);
    log_normal!(Info, "{}", output);
    log_normal!(Info, "{}", line);
    log_silent!(Info, "OK ({} Errors, {} Warnings)", assets.len() - succeeded_count, 0);

    assets.len() == succeeded_count
}


#[derive(Debug)]
struct Asset {
    pub name: String,
    pub kuid: String,
    pub path: PathBuf
}

fn locate_assets(build_path: &Path, recursive: bool) -> Vec<Asset> {
    let mut located_assets = Vec::new();
    locate_assets_recursive(build_path, recursive, &mut located_assets);
    located_assets
}

const KNOWN_DIRS: &'static [&'static str] = &[".git", ".hg"];

fn locate_assets_recursive(path: &Path, recursive: bool, located_assets: &mut Vec<Asset>) {

    log_verbose!(Info, "Entering directory: {0}", path.display());

    // First try to read config.txt file
    let config_path = path.join("config.txt");
    if let Ok(config_file) = File::open(&config_path) {
        // config.txt exists: read contents and try to find kuid
        log_verbose!(Info, "Found config.txt");
        let mut contents = String::new();
        BufReader::new(config_file).read_to_string(&mut contents).ok()
                .expect("unable to read config.txt");

        if let Some(caps) = KUID_MATCHER.captures(&contents) {
            // Found kuid - adding this path as asset root and exiting
            let kuid = caps.name("kuid").unwrap();

            let name = USERNAME_MATCHER.captures(&contents)
                    .and_then(|cap| cap.name("name"))
                    .unwrap_or("");

            log_normal!(Info, "Found asset '{}' <{}>: {}", name, kuid, path.display());

            located_assets.push(Asset {
                name: if name.is_empty() { format!("<{}>", kuid) } else { name.to_owned() },
                kuid: kuid.to_owned(),
                path: path.to_owned(),
            });
            return;
        }
    }
    // otherwise recursively walk all directories
    else if (recursive) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.metadata().unwrap().is_dir() {
                // Skip known directory names
                if entry.file_name().to_str().map(|s| KNOWN_DIRS.contains(&s)).unwrap_or(false) {
                    continue;
                }
                locate_assets_recursive(&entry.path(), true, located_assets);
            }
        }
    }
}


fn install_asset(asset: &Asset, args: &BuildArguments) -> bool {

    let asset_path: &Path = &asset.path;
    let asset_kuid: &str = &asset.kuid;
    let asset_name: &str  = &asset.name;

    log_normal!(Info, "Installing asset '{}'", asset_name);

    let result = ({
        match trainzutil::execute(args.trainzutil_path,
                &["installfrompath", asset_path.to_string_lossy().as_ref()]) {
            Ok(output) => {
                log_verbose!(Info, "Install susccess:\n{}", with_prefix(">", &output)); Ok(())
            },
            Err(e) => {
                log_normal!(Error, "Install failed: {}", e); Err(())
            }
        }
    }).and_then(|_| {
        match trainzutil::execute(args.trainzutil_path, &["commit", asset_kuid]) {
            Ok(output) => {
                log_verbose!(Info, "Commit success:\n{}", with_prefix(">", &output)); Ok(())
            },
            Err(e) => {
                log_normal!(Error, "Commit failed: {}", e); Err(())
            }
        }
    }).and_then(|_| {
        log_verbose!(Info, "Install success");
        Ok(())
    }).or_else(|_| {
        log_normal!(Error, "Install failed");
        Err(())
    });

    result.is_ok()
}


fn validate_asset(asset: &Asset, args: &BuildArguments) -> bool {

    let asset_path: &Path = &asset.path;
    let asset_kuid: &str = &asset.kuid;
    let asset_name: &str  = &asset.name;

    let asset_relative_path = &{
        let comps = asset_path.components().skip(args.build_path.components().count());
        let mut buf = PathBuf::new();
        for c in comps {
            match c {
                path::Component::Normal(p) => buf.push(p),
                _ => panic!("unexpected path component")
            }
        }
        buf
    };
    let asset_output_name = match (args.show_config_path, args.show_kuid) {
        (true, _) => format!("[{}]", asset_relative_path.join("config.txt").to_string_lossy().as_ref()),
        (_, true) => format!("<{}>", asset_kuid),
        (_, _)    => format!("[{}]", asset_relative_path.to_string_lossy().as_ref()),
    };

    log_normal!(Info, "Validating asset '{}'", asset_name);

    let result = ({
        match trainzutil::execute(args.trainzutil_path, &["validate", asset_kuid]) {
            Ok(output) => {
                log_verbose!(Info, "Validation success:\n{}", with_prefix(">", &output));
                log_validation_output(&asset_output_name, &output);
                if output.errors == 0 {
                    Ok(())
                } else {
                    log_verbose!(Error, "Asset is not valid"); Err(())
                }
            }
            Err(e) => {
                log_normal!(Error, "Validation failed: {}", e); Err(())
            }
        }
    }).and_then(|_| {
        log_verbose!(Info, "Validation success");
        Ok(())
    }).or_else(|_| {
        log_normal!(Error, "Validation failed");
        Err(())
    });

    result.is_ok()
}


fn log_validation_output(asset: &str, output: &trainzutil::Output) {
    for line in &output.lines {
        if let Some(caps) = trainzutil::TZUTIL_OUTPUT_MATCHER.captures(line) {
            let prefix = caps.name("prefix").unwrap();
            let message = caps.name("message").unwrap();
            match prefix {
                "-" => log_normal! (Error, "{}", message),
                "!" => log_normal! ( Warn, "{}", message),
                "+" => log_normal! ( Info, "{}", message),
                ";" => log_verbose!( Info, "{}", message),
                 _   => unreachable!()
            }
            match prefix {
                "-" => log_silent!(Error, "{} {} : {}", prefix, asset, message),
                "!" => log_silent!( Warn, "{} {} : {}", prefix, asset, message),
                "+" => log_silent!( Info, "{} {} : {}", prefix, asset, message),
                ";" => log_silent!( Info, "{} {} : {}", prefix, asset, message),
                 _   => unreachable!()
            }
        }
    }
}
