pub mod command;
mod graph;

use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::{env, fmt, io, process};

use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{Message, MetadataCommand};
use rustc_version::Channel;
use semver::Version;
use serde::Deserialize;
use tee::TeeReader;

use crate::command::{CargoCmd, Input, Run, Test};
use crate::graph::UnitGraph;

/// Build a command using [`make_cargo_build_command`] and execute it,
/// parsing and returning the messages from the spawned process.
///
/// For commands that produce an executable output, this function will build the
/// `.elf` binary that can be used to create other 3ds files.
pub fn run_cargo(input: &Input, message_format: Option<String>) -> (ExitStatus, Vec<Message>) {
    let mut command = make_cargo_command(input, &message_format);

    // The unit graph is needed only when compiling a program.
    if input.cmd.should_compile() {
        let libctru = if should_use_ctru_debuginfo(&command, input.verbose) {
            "ctrud"
        } else {
            "ctru"
        };

        let rustflags = command
            .get_envs()
            .find(|(var, _)| var == &OsStr::new("RUSTFLAGS"))
            .and_then(|(_, flags)| flags)
            .unwrap_or_default()
            .to_string_lossy();

        let rustflags = format!("{rustflags} -l{libctru}");

        command.env("RUSTFLAGS", rustflags);
    }

    if input.verbose {
        print_command(&command);
    }

    let mut process = command.spawn().unwrap();
    let command_stdout = process.stdout.take().unwrap();

    let mut tee_reader;
    let mut stdout_reader;

    let buf_reader: &mut dyn BufRead = match (message_format, &input.cmd) {
        // The user presumably cares about the message format if set, so we should
        // copy stuff to stdout like they expect. We can still extract the executable
        // information out of it that we need for 3dsxtool etc.
        (Some(_), _) |
        // Rustdoc unfortunately prints to stdout for compile errors, so
        // we also use a tee when building doc tests too.
        // Possibly related: https://github.com/rust-lang/rust/issues/75135
        (None, CargoCmd::Test(Test { doc: true, .. })) => {
            tee_reader = BufReader::new(TeeReader::new(command_stdout, io::stdout()));
            &mut tee_reader
        }
        _ => {
            stdout_reader = BufReader::new(command_stdout);
            &mut stdout_reader
        }
    };

    let messages = Message::parse_stream(buf_reader)
        .collect::<io::Result<_>>()
        .unwrap();

    (process.wait().unwrap(), messages)
}

/// Ensure that we use the same `-lctru[d]` flag that `ctru-sys` is using in its build.
fn should_use_ctru_debuginfo(cargo_cmd: &Command, verbose: bool) -> bool {
    match UnitGraph::from_cargo(cargo_cmd, verbose) {
        Ok(unit_graph) => {
            let Some(unit) = unit_graph
                .units
                .iter()
                .find(|unit| unit.target.name == "ctru-sys")
            else {
                eprintln!("Warning: unable to check if `ctru` debuginfo should be linked: `ctru-sys` not found");
                return false;
            };

            let debuginfo = unit.profile.debuginfo.unwrap_or(0);
            debuginfo > 0
        }
        Err(err) => {
            eprintln!("Warning: unable to check if `ctru` debuginfo should be linked: {err}");
            false
        }
    }
}

/// Create a cargo command based on the context.
///
/// For "build" commands (which compile code, such as `cargo 3ds build` or `cargo 3ds clippy`),
/// if there is no pre-built std detected in the sysroot, `build-std` will be used instead.
pub fn make_cargo_command(input: &Input, message_format: &Option<String>) -> Command {
    let devkitpro =
        env::var("DEVKITPRO").expect("DEVKITPRO is not defined as an environment variable");
    // TODO: should we actually prepend the user's RUSTFLAGS for linking order? not sure
    let rustflags =
        env::var("RUSTFLAGS").unwrap_or_default() + &format!(" -L{devkitpro}/libctru/lib");

    let cargo_cmd = &input.cmd;

    let mut command = cargo(&input.config);
    command
        .arg(cargo_cmd.subcommand_name())
        .env("RUSTFLAGS", rustflags);

    // Any command that needs to compile code will run under this environment.
    // Even `clippy` and `check` need this kind of context, so we'll just assume any other `Passthrough` command uses it too.
    if cargo_cmd.should_compile() {
        command
            .arg("--target")
            .arg("armv6k-nintendo-3ds")
            .arg("--message-format")
            .arg(
                message_format
                    .as_deref()
                    .unwrap_or(CargoCmd::DEFAULT_MESSAGE_FORMAT),
            );

        let sysroot = find_sysroot();
        if !sysroot.join("lib/rustlib/armv6k-nintendo-3ds").exists() {
            eprintln!("No pre-build std found, using build-std");
            // Always building the test crate is not ideal, but we don't know if the
            // crate being built uses #![feature(test)], so we build it just in case.
            command.arg("-Z").arg("build-std=std,test");
        }
    }

    if let CargoCmd::Test(test) = cargo_cmd {
        // RUSTDOCFLAGS is simply ignored if --doc wasn't passed, so we always set it.
        let rustdoc_flags = std::env::var("RUSTDOCFLAGS").unwrap_or_default() + test.rustdocflags();
        command.env("RUSTDOCFLAGS", rustdoc_flags);
    }

    command.args(cargo_cmd.cargo_args());

    if let CargoCmd::Run(run) | CargoCmd::Test(Test { run_args: run, .. }) = &cargo_cmd {
        if run.use_custom_runner() {
            command
                .arg("--")
                .args(run.build_args.passthrough.exe_args());
        }
    }

    command
        .stdout(Stdio::piped())
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit());

    command
}

/// Build a `cargo` command with the given `--config` flags.
fn cargo(config: &[String]) -> Command {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.args(config.iter().map(|cfg| format!("--config={cfg}")));
    cmd
}

fn print_command(command: &Command) {
    let mut cmd_str = vec![command.get_program().to_string_lossy().to_string()];
    cmd_str.extend(command.get_args().map(|s| s.to_string_lossy().to_string()));

    eprintln!("Running command:");
    for (k, v) in command.get_envs() {
        let v = v.map(|v| v.to_string_lossy().to_string());
        eprintln!(
            "   {}={} \\",
            k.to_string_lossy(),
            v.map_or_else(String::new, |s| shlex::try_quote(&s).unwrap().to_string())
        );
    }
    eprintln!(
        "   {}\n",
        shlex::try_join(cmd_str.iter().map(String::as_str)).unwrap()
    );
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
pub fn check_rust_version(input: &Input) {
    let rustc_version = rustc_version::version_meta().unwrap();

    // If the channel isn't nightly, we can't make use of the required unstable tools.
    // However, `cargo 3ds new` doesn't have these requirements.
    if rustc_version.channel > Channel::Nightly && input.cmd.should_compile() {
        eprintln!("building with cargo-3ds requires a nightly rustc version.");
        eprintln!(
            "Please run `rustup override set nightly` to use nightly in the \
            current directory, or use `cargo +nightly 3ds` to use it for a \
            single invocation."
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
        eprintln!("cargo-3ds requires rustc nightly version >= {MINIMUM_COMMIT_DATE}");
        eprintln!("Please run `rustup update nightly` to upgrade your nightly version");

        process::exit(1);
    }
}

/// Parses messages returned by "build" cargo commands (such as `cargo 3ds build` or `cargo 3ds run`).
/// The returned [`CTRConfig`] is then used for further building in and execution
/// in [`build_smdh`], [`build_3dsx`], and [`link`].
pub fn get_metadata(messages: &[Message]) -> CTRConfig {
    let metadata = MetadataCommand::new()
        .no_deps()
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

    let config = package
        .metadata
        .get("cargo-3ds")
        .and_then(|c| CTRConfig::deserialize(c).ok())
        .unwrap_or_default();

    CTRConfig {
        name,
        authors: config.authors.or(Some(package.authors)),
        description: config.description.or(package.description),
        manifest_dir: package.manifest_path.parent().unwrap().into(),
        target_path: artifact.executable.unwrap(),
        ..config
    }
}

/// Builds the 3dsx using `3dsxtool`.
/// This will fail if `3dsxtool` is not within the running directory or in a directory found in $PATH
pub fn build_3dsx(config: &CTRConfig, verbose: bool) {
    let mut command = Command::new("3dsxtool");
    command
        .arg(&config.target_path)
        .arg(config.path_3dsx())
        .arg(format!("--smdh={}", config.path_smdh()));

    let romfs = config.romfs_dir();
    if romfs.is_dir() {
        eprintln!("Adding RomFS from {romfs}");
        command.arg(format!("--romfs={romfs}"));
    } else if config.romfs_dir.is_some() {
        eprintln!("Could not find configured RomFS dir: {romfs}");
        process::exit(1);
    }

    if verbose {
        print_command(&command);
    }

    let mut process = command
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
pub fn link(config: &CTRConfig, run_args: &Run, verbose: bool) {
    let mut command = Command::new("3dslink");
    command
        .arg(config.path_3dsx())
        .args(run_args.get_3dslink_args())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if verbose {
        print_command(&command);
    }

    let status = command.spawn().unwrap().wait().unwrap();

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

#[derive(Default, Debug, Deserialize, PartialEq, Eq)]
pub struct CTRConfig {
    /// The authors of the application, which will be joined by `", "` to form
    /// the `Publisher` field in the SMDH format. If not specified, a single author
    /// of "Unspecified Author" will be used.
    authors: Option<Vec<String>>,

    /// A description of the application, also called `Long Description` in the
    /// SMDH format. The following values will be used in order of precedence:
    /// - `cargo-3ds` metadata field
    /// - `package.description` in Cargo.toml
    /// - "Homebrew Application"
    description: Option<String>,

    /// The path to the app icon, defaulting to `$CARGO_MANIFEST_DIR/icon.png`
    /// if it exists. If not specified, the devkitPro default icon is used.
    icon_path: Option<Utf8PathBuf>,

    /// The path to the romfs directory, defaulting to `$CARGO_MANIFEST_DIR/romfs`
    /// if it exists, or unused otherwise. If a path is specified but does not
    /// exist, an error occurs.
    #[serde(alias = "romfs-dir")]
    romfs_dir: Option<Utf8PathBuf>,

    // Remaining fields come from cargo metadata / build artifact output and
    // cannot be customized by users in `package.metadata.cargo-3ds`. I suppose
    // in theory we could allow name to be customizable if we wanted...
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    target_path: Utf8PathBuf,
    #[serde(skip)]
    manifest_dir: Utf8PathBuf,
}

impl CTRConfig {
    /// Get the path to the output `.3dsx` file.
    pub fn path_3dsx(&self) -> Utf8PathBuf {
        self.target_path.with_extension("3dsx")
    }

    /// Get the path to the output `.smdh` file.
    pub fn path_smdh(&self) -> Utf8PathBuf {
        self.target_path.with_extension("smdh")
    }

    /// Get the absolute path to the romfs directory, defaulting to `romfs` if not specified.
    pub fn romfs_dir(&self) -> Utf8PathBuf {
        self.manifest_dir
            .join(self.romfs_dir.as_deref().unwrap_or(Utf8Path::new("romfs")))
    }

    // as standard with the devkitPRO toolchain
    const DEFAULT_AUTHOR: &'static str = "Unspecified Author";
    const DEFAULT_DESCRIPTION: &'static str = "Homebrew Application";

    /// Builds the smdh using `smdhtool`.
    /// This will fail if `smdhtool` is not within the running directory or in a directory found in $PATH
    pub fn build_smdh(&self, verbose: bool) {
        let description = self
            .description
            .as_deref()
            .unwrap_or(Self::DEFAULT_DESCRIPTION);

        let publisher = if let Some(authors) = self.authors.as_ref() {
            authors.join(", ")
        } else {
            Self::DEFAULT_AUTHOR.to_string()
        };

        let mut command = Command::new("smdhtool");
        command
            .arg("--create")
            .arg(&self.name)
            .arg(description)
            .arg(publisher)
            .arg(self.icon_path())
            .arg(self.path_smdh())
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        if verbose {
            print_command(&command);
        }

        let mut process = command
            .spawn()
            .expect("smdhtool command failed, most likely due to 'smdhtool' not being in $PATH");

        let status = process.wait().unwrap();

        if !status.success() {
            process::exit(status.code().unwrap_or(1));
        }
    }

    /// Possible cases:
    /// - icon path specified (exit with error if doesn't exist)
    /// - icon path unspecified, icon.png exists
    /// - icon path unspecified, icon.png does not exist
    fn icon_path(&self) -> Utf8PathBuf {
        let abs_path = self.manifest_dir.join(
            self.icon_path
                .as_deref()
                .unwrap_or(Utf8Path::new("icon.png")),
        );

        if abs_path.is_file() {
            abs_path
        } else if self.icon_path.is_some() {
            eprintln!("Specified icon path does not exist: {abs_path}");
            process::exit(1);
        } else {
            // We assume this default icon will always exist as part of the toolchain
            Utf8PathBuf::from(env::var("DEVKITPRO").unwrap())
                .join("libctru")
                .join("default_icon.png")
        }
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
    year: 2023,
    month: 5,
    day: 31,
};
const MINIMUM_RUSTC_VERSION: Version = Version::new(1, 70, 0);
