extern crate core;

pub mod command;

use crate::command::{CargoCommand, Input};
use cargo_metadata::{Message, MetadataCommand};
use core::fmt;
use rustc_version::Channel;
use semver::Version;
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::{env, io, process};
use tee::TeeReader;

const DEFAULT_MESSAGE_FORMAT: &str = "json-render-diagnostics";

/// Gets whether the executable should link the generated files to a 3ds
/// based on the parsed input.
pub fn get_should_link(input: &mut Input) -> bool {
    // When running compile only commands, don't link the executable to the 3ds.
    // Otherwise, link and run on the 3ds but do not run locally.
    input.cmd == CargoCommand::Build
        || (input.cmd == CargoCommand::Test
            && if input.cargo_opts.contains(&"--no-run".to_string()) {
                false
            } else {
                input.cargo_opts.push("--no-run".to_string());
                true
            })
}

/// Extracts the user-defined message format and if there is none,
/// default to `json-render-diagnostics`.
pub fn get_message_format(input: &mut Input) -> String {
    // Checks for a position within the args where '--message-format' is located
    if let Some(pos) = input
        .cargo_opts
        .iter()
        .position(|s| s.starts_with("--message-format"))
    {
        // Remove the arg from list
        let arg = input.cargo_opts.remove(pos);

        // Allows for usage of '--message-format=<format>' and also using space separation.
        // Check for a '=' delimiter and use the second half of the split as the format,
        // otherwise remove next arg which is now at the same position as the original flag.
        let format = if let Some((_, format)) = arg.split_once('=') {
            format.to_string()
        } else {
            input.cargo_opts.remove(pos)
        };

        // Non-json formats are not supported so the executable exits.
        if !format.starts_with("json") {
            eprintln!("error: non-JSON `message-format` is not supported");
            process::exit(1);
        } else {
            format
        }
    } else {
        // Default to 'json-render-diagnostics'
        DEFAULT_MESSAGE_FORMAT.to_string()
    }
}

/// Build the elf that will be used to create other 3ds files.
/// The command built from [`make_cargo_build_command`] is executed
/// and the messages from the spawned process are parsed and returned.
pub fn build_elf(
    cmd: CargoCommand,
    message_format: &str,
    args: &Vec<String>,
) -> (ExitStatus, Vec<Message>) {
    let mut command = make_cargo_build_command(cmd, message_format, args);
    let mut process = command.spawn().unwrap();
    let command_stdout = process.stdout.take().unwrap();

    let mut tee_reader;
    let mut stdout_reader;
    let buf_reader: &mut dyn BufRead = if message_format == DEFAULT_MESSAGE_FORMAT {
        stdout_reader = BufReader::new(command_stdout);
        &mut stdout_reader
    } else {
        // The user presumably cares about the message format, so we should
        // copy stuff to stdout like they expect. We can still extract the executable
        // information out of it that we need for 3dsxtool etc.
        tee_reader = BufReader::new(TeeReader::new(command_stdout, io::stdout()));
        &mut tee_reader
    };

    let messages = Message::parse_stream(buf_reader)
        .collect::<io::Result<_>>()
        .unwrap();

    (process.wait().unwrap(), messages)
}

/// Create the cargo build command, but don't execute it.
/// If there is no pre-built std detected in the sysroot, `build-std` is used.
pub fn make_cargo_build_command(
    cmd: CargoCommand,
    message_format: &str,
    args: &Vec<String>,
) -> Command {
    let rust_flags = env::var("RUSTFLAGS").unwrap_or_default()
        + &format!(
            " -L{}/libctru/lib -lctru",
            env::var("DEVKITPRO").expect("DEVKITPRO is not defined as an environment variable")
        );
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let sysroot = find_sysroot();
    let mut command = Command::new(cargo);

    if !sysroot.join("lib/rustlib/armv6k-nintendo-3ds").exists() {
        eprintln!("No pre-build std found, using build-std");
        command.arg("-Z").arg("build-std");
    }

    command
        .env("RUSTFLAGS", rust_flags)
        .arg(&cmd.to_string())
        .arg("--target")
        .arg("armv6k-nintendo-3ds")
        .arg("--message-format")
        .arg(message_format)
        .args(args)
        .stdout(Stdio::piped())
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit());

    command
}

/// Finds the sysroot path of the current toolchain
pub fn find_sysroot() -> PathBuf {
    let sysroot = env::var("SYSROOT").ok().unwrap_or_else(|| {
        let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());

        let output = Command::new(&rustc)
            .arg("--print")
            .arg("sysroot")
            .output()
            .unwrap_or_else(|_| panic!("Failed to run `{rustc} -- print sysroot`"));
        String::from_utf8(output.stdout).expect("Failed to parse sysroot path into a UTF-8 string")
    });

    PathBuf::from(sysroot.trim())
}

/// Checks the current rust version and channel.
/// Exits if the minimum requirement is not met.
pub fn check_rust_version() {
    let rustc_version = rustc_version::version_meta().unwrap();

    if rustc_version.channel > Channel::Nightly {
        eprintln!("cargo-3ds requires a nightly rustc version.");
        eprintln!(
            "Please run `rustup override set nightly` to use nightly in the \
            current directory."
        );
        process::exit(1);
    }

    let old_version = MINIMUM_RUSTC_VERSION
        > Version {
            // Remove `-nightly` pre-release tag for comparison.
            pre: semver::Prerelease::EMPTY,
            ..rustc_version.semver.clone()
        };

    let old_commit = match rustc_version.commit_date {
        None => false,
        Some(date) => {
            MINIMUM_COMMIT_DATE
                > CommitDate::parse(&date).expect("could not parse `rustc --version` commit date")
        }
    };

    if old_version || old_commit {
        eprintln!(
            "cargo-3ds requires rustc nightly version >= {}",
            MINIMUM_COMMIT_DATE,
        );
        eprintln!("Please run `rustup update nightly` to upgrade your nightly version");

        process::exit(1);
    }
}

/// Parses messages returned by the executed cargo command from [`build_elf`].
/// The returned [`CTRConfig`] is then used for further building in and execution
/// in [`build_smdh`], ['build_3dsx'], and [`link`].
pub fn get_metadata(messages: &[Message]) -> CTRConfig {
    let metadata = MetadataCommand::new()
        .exec()
        .expect("Failed to get cargo metadata");

    let mut package = None;
    let mut artifact = None;

    // Extract the final built executable. We may want to fail in cases where
    // multiple executables, or none, were built?
    for message in messages.iter().rev() {
        if let Message::CompilerArtifact(art) = message {
            if art.executable.is_some() {
                package = Some(metadata[&art.package_id].clone());
                artifact = Some(art.clone());

                break;
            }
        }
    }
    if package.is_none() || artifact.is_none() {
        eprintln!("No executable found from build command output!");
        process::exit(1);
    }

    let (package, artifact) = (package.unwrap(), artifact.unwrap());

    let mut icon = String::from("./icon.png");

    if !Path::new(&icon).exists() {
        icon = format!(
            "{}/libctru/default_icon.png",
            env::var("DEVKITPRO").unwrap()
        );
    }

    // for now assume a single "kind" since we only support one output artifact
    let name = match artifact.target.kind[0].as_ref() {
        "bin" | "lib" | "rlib" | "dylib" if artifact.target.test => {
            format!("{} tests", artifact.target.name)
        }
        "example" => {
            format!("{} - {} example", artifact.target.name, package.name)
        }
        _ => artifact.target.name,
    };

    let author = match package.authors.as_slice() {
        [name, ..] => name.to_owned(),
        [] => String::from("Unspecified Author"), // as standard with the devkitPRO toolchain
    };

    CTRConfig {
        name,
        author,
        description: package
            .description
            .clone()
            .unwrap_or_else(|| String::from("Homebrew Application")),
        icon,
        target_path: artifact.executable.unwrap().into(),
        cargo_manifest_path: package.manifest_path.into(),
    }
}

/// Builds the smdh using `smdhtool`.
/// This will fail if `smdhtool` is not within the running directory or in a directory found in $PATH
pub fn build_smdh(config: &CTRConfig) {
    let mut process = Command::new("smdhtool")
        .arg("--create")
        .arg(&config.name)
        .arg(&config.description)
        .arg(&config.author)
        .arg(&config.icon)
        .arg(config.path_smdh())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("smdhtool command failed, most likely due to 'smdhtool' not being in $PATH");

    let status = process.wait().unwrap();

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

/// Builds the 3dsx using `3dsxtool`.
/// This will fail if `3dsxtool` is not within the running directory or in a directory found in $PATH
pub fn build_3dsx(config: &CTRConfig) {
    let mut command = Command::new("3dsxtool");
    let mut process = command
        .arg(&config.target_path)
        .arg(config.path_3dsx())
        .arg(format!("--smdh={}", config.path_smdh().to_string_lossy()));

    // If romfs directory exists, automatically include it
    let (romfs_path, is_default_romfs) = get_romfs_path(config);
    if romfs_path.is_dir() {
        eprintln!("Adding RomFS from {}", romfs_path.display());
        process = process.arg(format!("--romfs={}", romfs_path.to_string_lossy()));
    } else if !is_default_romfs {
        eprintln!(
            "Could not find configured RomFS dir: {}",
            romfs_path.display()
        );
        process::exit(1);
    }

    let mut process = process
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("3dsxtool command failed, most likely due to '3dsxtool' not being in $PATH");

    let status = process.wait().unwrap();

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

/// Link the generated 3dsx to a 3ds to execute and test using `3dslink`.
/// This will fail if `3dslink` is not within the running directory or in a directory found in $PATH
pub fn link(config: &CTRConfig) {
    let mut process = Command::new("3dslink")
        .arg(config.path_3dsx())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    let status = process.wait().unwrap();

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

/// Read the `RomFS` path from the Cargo manifest. If it's unset, use the default.
/// The returned boolean is true when the default is used.
pub fn get_romfs_path(config: &CTRConfig) -> (PathBuf, bool) {
    let manifest_path = &config.cargo_manifest_path;
    let manifest_str = std::fs::read_to_string(manifest_path)
        .unwrap_or_else(|e| panic!("Could not open {}: {e}", manifest_path.display()));
    let manifest_data: toml::Value =
        toml::de::from_str(&manifest_str).expect("Could not parse Cargo manifest as TOML");

    // Find the romfs setting and compute the path
    let mut is_default = false;
    let romfs_dir_setting = manifest_data
        .as_table()
        .and_then(|table| table.get("package"))
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("metadata"))
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("cargo-3ds"))
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("romfs_dir"))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| {
            is_default = true;
            "romfs"
        });
    let mut romfs_path = manifest_path.clone();
    romfs_path.pop(); // Pop Cargo.toml
    romfs_path.push(romfs_dir_setting);

    (romfs_path, is_default)
}

#[derive(Deserialize, Default)]
pub struct CTRConfig {
    name: String,
    author: String,
    description: String,
    icon: String,
    target_path: PathBuf,
    cargo_manifest_path: PathBuf,
}

impl CTRConfig {
    pub fn path_3dsx(&self) -> PathBuf {
        self.target_path.with_extension("3dsx")
    }

    pub fn path_smdh(&self) -> PathBuf {
        self.target_path.with_extension("smdh")
    }
}

#[derive(Ord, PartialOrd, PartialEq, Eq, Debug)]
pub struct CommitDate {
    year: i32,
    month: i32,
    day: i32,
}

impl CommitDate {
    fn parse(date: &str) -> Option<Self> {
        let mut iter = date.split('-');

        let year = iter.next()?.parse().ok()?;
        let month = iter.next()?.parse().ok()?;
        let day = iter.next()?.parse().ok()?;

        Some(Self { year, month, day })
    }
}

impl fmt::Display for CommitDate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

const MINIMUM_COMMIT_DATE: CommitDate = CommitDate {
    year: 2022,
    month: 6,
    day: 15,
};
const MINIMUM_RUSTC_VERSION: Version = Version::new(1, 63, 0);
