use cargo_metadata::{Message, MetadataCommand};
use rustc_version::{Channel, Version};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::{
    env, fmt, io,
    process::{self, Command, Stdio},
};
use tee::TeeReader;

#[derive(serde_derive::Deserialize, Default)]
struct CTRConfig {
    name: String,
    author: String,
    description: String,
    icon: String,
    target_path: PathBuf,
    cargo_manifest_path: PathBuf,
}

impl CTRConfig {
    fn path_3dsx(&self) -> PathBuf {
        self.target_path.with_extension("3dsx")
    }

    fn path_smdh(&self) -> PathBuf {
        self.target_path.with_extension("smdh")
    }
}

#[derive(Ord, PartialOrd, PartialEq, Eq, Debug)]
struct CommitDate {
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
    year: 2021,
    month: 10,
    day: 1,
};
const MINIMUM_RUSTC_VERSION: Version = Version::new(1, 56, 0);

fn main() {
    check_rust_version();

    if env::args().any(|arg| arg == "--help" || arg == "-h") {
        print_usage(&mut io::stdout());
        return;
    }

    // Get the command and collect the remaining arguments
    let cargo_command = CargoCommand::from_args().unwrap_or_else(|| {
        print_usage(&mut io::stderr());
        process::exit(2)
    });

    eprintln!("Running Cargo");
    let (status, messages) = cargo_command.build_elf();
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    if !cargo_command.should_build_3dsx() {
        return;
    }

    eprintln!("Getting metadata");
    let app_conf = get_metadata(&messages);

    eprintln!("Building smdh:{}", app_conf.path_smdh().display());
    build_smdh(&app_conf);

    eprintln!("Building 3dsx: {}", app_conf.path_3dsx().display());
    build_3dsx(&app_conf);

    if cargo_command.should_link {
        eprintln!("Running 3dslink");
        link(&app_conf);
    }
}

struct CargoCommand {
    command: String,
    should_link: bool,
    args: Vec<String>,
    message_format: String,
}

impl CargoCommand {
    const DEFAULT_MESSAGE_FORMAT: &'static str = "json-render-diagnostics";

    fn from_args() -> Option<Self> {
        // Skip `cargo 3ds`. `cargo-3ds` isn't supported for now
        let mut args = env::args().skip(2);

        let command = args.next()?;
        let mut remaining_args: Vec<String> = args.collect();

        let (command, should_link) = match command.as_str() {
            "run" => ("build".to_string(), true),
            "test" => {
                let no_run = String::from("--no-run");

                if remaining_args.contains(&no_run) {
                    (command, false)
                } else {
                    remaining_args.push(no_run);
                    (command, true)
                }
            }
            _ => (command, false),
        };

        let message_format = match Self::extract_message_format(&mut remaining_args) {
            Some(format) => {
                if !format.starts_with("json") {
                    eprintln!("error: non-JSON `message-format` is not supported");
                    process::exit(1);
                }
                format
            }
            None => Self::DEFAULT_MESSAGE_FORMAT.to_string(),
        };

        Some(Self {
            command,
            should_link,
            args: remaining_args,
            message_format,
        })
    }

    fn extract_message_format(args: &mut Vec<String>) -> Option<String> {
        for (i, arg) in args.iter().enumerate() {
            if arg.starts_with("--message-format") {
                return {
                    let arg = args.remove(i);

                    if let Some((_, format)) = arg.split_once('=') {
                        Some(format.to_string())
                    } else {
                        Some(args.remove(i))
                    }
                };
            }
        }

        None
    }

    fn build_elf(&self) -> (ExitStatus, Vec<Message>) {
        let mut command = self.make_cargo_build_command();
        let mut process = command.spawn().unwrap();
        let command_stdout = process.stdout.take().unwrap();

        let mut tee_reader;
        let mut stdout_reader;

        let buf_reader: &mut dyn BufRead = if self.message_format == Self::DEFAULT_MESSAGE_FORMAT {
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
    fn make_cargo_build_command(&self) -> Command {
        let rustflags = env::var("RUSTFLAGS").unwrap_or_default()
            + &format!(" -L{}/libctru/lib -lctru", env::var("DEVKITPRO").unwrap());
        let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let sysroot = Self::find_sysroot();
        let mut command = Command::new(cargo);

        if !sysroot.join("lib/rustlib/armv6k-nintendo-3ds").exists() {
            eprintln!("No pre-built std found, using build-std");
            command.arg("-Z").arg("build-std");
        }

        command
            .env("RUSTFLAGS", rustflags)
            .arg(&self.command)
            .arg("--target")
            .arg("armv6k-nintendo-3ds")
            .arg("--message-format")
            .arg(&self.message_format)
            .args(&self.args)
            .stdout(Stdio::piped())
            .stdin(Stdio::inherit())
            .stderr(Stdio::inherit());

        command
    }

    /// Get the compiler's sysroot path
    fn find_sysroot() -> PathBuf {
        let sysroot = env::var("SYSROOT").ok().unwrap_or_else(|| {
            // Get sysroot from rustc
            let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());

            let output = Command::new(&rustc)
                .arg("--print")
                .arg("sysroot")
                .output()
                .unwrap_or_else(|_| panic!("Failed to run `{rustc} --print sysroot`"));

            String::from_utf8(output.stdout)
                .expect("Failed to parse sysroot path into a UTF-8 string")
        });

        PathBuf::from(sysroot.trim())
    }

    fn should_build_3dsx(&self) -> bool {
        matches!(self.command.as_str(), "build" | "run" | "test")
    }
}

fn print_usage(f: &mut impl io::Write) {
    let invocation = {
        let mut args = env::args();

        // We do this to properly display `cargo-3ds` if invoked that way
        let bin = args.next().unwrap();
        if let Some("3ds") = args.next().as_deref() {
            "cargo 3ds".to_string()
        } else {
            bin
        }
    };

    writeln!(
        f,
        "{name}: {description}.

Usage:
    {invocation} build [CARGO_OPTS...]
    {invocation} run [CARGO_OPTS...]
    {invocation} test [CARGO_OPTS...]
    {invocation} <cargo-command> [CARGO_OPTS...]
    {invocation} -h | --help

Commands:
    build           build a 3dsx executable.
    run             build a 3dsx executable and send it to a device with 3dslink.
    test            build a 3dsx executable from unit/integration tests and send it to a device.
    <cargo-command> execute some other Cargo command with 3ds options configured (ex. check or clippy).

Options:
    -h --help       Show this screen.

Additional arguments will be passed through to `<cargo-command>`. Some that are supported include:

    [build | run | test] --release
    test --no-run

Other flags may work, but haven't been tested.
",
        name = env!("CARGO_BIN_NAME"),
        description = env!("CARGO_PKG_DESCRIPTION"),
        invocation = invocation,
    )
    .unwrap();
}

fn check_rust_version() {
    let rustc_version = rustc_version::version_meta().unwrap();

    if rustc_version.channel > Channel::Nightly {
        eprintln!("cargo-3ds requires a nightly rustc version.");
        eprintln!(
            "Please run `rustup override set nightly` to use nightly in the \
            current directory."
        );
        process::exit(1);
    }

    let old_version: bool = MINIMUM_RUSTC_VERSION > rustc_version.semver;

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

fn get_metadata(messages: &[Message]) -> CTRConfig {
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

fn build_smdh(config: &CTRConfig) {
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
        .unwrap();

    let status = process.wait().unwrap();

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

fn build_3dsx(config: &CTRConfig) {
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
        .unwrap();

    let status = process.wait().unwrap();

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

fn link(config: &CTRConfig) {
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
fn get_romfs_path(config: &CTRConfig) -> (PathBuf, bool) {
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
