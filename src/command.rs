use std::fs;
use std::io::Read;
use std::process::{self, Stdio};
use std::sync::OnceLock;

use cargo_metadata::{Message, Metadata};
use clap::{Args, Parser, Subcommand};

use crate::{build_3dsx, cargo, get_artifact_config, link, print_command, CTRConfig};

#[derive(Parser, Debug)]
#[command(name = "cargo", bin_name = "cargo")]
pub enum Cargo {
    #[command(name = "3ds")]
    Input(Input),
}

#[derive(Args, Debug)]
#[command(version, about)]
pub struct Input {
    #[command(subcommand)]
    pub cmd: CargoCmd,

    /// Print the exact commands `cargo-3ds` is running. Note that this does not
    /// set the verbose flag for cargo itself. To set cargo's verbosity flag, add
    /// `-- -v` to the end of the command line.
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    /// Set cargo configuration on the command line. This is equivalent to
    /// cargo's `--config` option.
    #[arg(long, global = true)]
    pub config: Vec<String>,
}

/// Run a cargo command. COMMAND will be forwarded to the real
/// `cargo` with the appropriate arguments for the 3DS target.
///
/// If an unrecognized COMMAND is used, it will be passed through unmodified
/// to `cargo` with the appropriate flags set for the 3DS target.
#[derive(Subcommand, Debug)]
#[command(allow_external_subcommands = true)]
pub enum CargoCmd {
    /// Builds an executable suitable to run on a 3DS (3dsx).
    Build(Build),

    /// Builds an executable and sends it to a device with `3dslink`.
    Run(Run),

    /// Builds a test executable and sends it to a device with `3dslink`.
    ///
    /// This can be used with `--test` for integration tests, or `--lib` for
    /// unit tests (which require a custom test runner).
    Test(Test),

    /// Sets up a new cargo project suitable to run on a 3DS.
    New(New),

    // NOTE: it seems docstring + name for external subcommands are not rendered
    // in help, but we might as well set them here in case a future version of clap
    // does include them in help text.
    /// Run any other `cargo` command with custom building tailored for the 3DS.
    #[command(external_subcommand, name = "COMMAND")]
    Passthrough(Vec<String>),
}

#[derive(Args, Debug)]
pub struct RemainingArgs {
    /// Pass additional options through to the `cargo` command.
    ///
    /// All arguments after the first `--`, or starting with the first unrecognized
    /// option, will be passed through to `cargo` unmodified.
    ///
    /// To pass arguments to an executable being run, a *second* `--` must be
    /// used to disambiguate cargo arguments from executable arguments.
    /// For example, `cargo 3ds run -- -- xyz` runs an executable with the argument
    /// `xyz`.
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CARGO_ARGS"
    )]
    args: Vec<String>,
}

#[allow(unused_variables)]
trait Callbacks {
    fn build_callback(&self, config: &CTRConfig) {}
    fn run_callback(&self, config: &CTRConfig) {}
}

#[derive(Args, Debug)]
pub struct Build {
    #[arg(from_global)]
    pub verbose: bool,

    // Passthrough cargo options.
    #[command(flatten)]
    pub passthrough: RemainingArgs,
}

#[derive(Args, Debug)]
pub struct Run {
    /// Specify the IP address of the device to send the executable to.
    ///
    /// Corresponds to 3dslink's `--address` arg, which defaults to automatically
    /// finding the device.
    #[arg(long, short = 'a')]
    pub address: Option<std::net::Ipv4Addr>,

    /// Set the 0th argument of the executable when running it. Corresponds to
    /// 3dslink's `--argv0` argument.
    #[arg(long, short = '0')]
    pub argv0: Option<String>,

    /// Start the 3dslink server after sending the executable. Corresponds to
    /// 3dslink's `--server` argument.
    #[arg(long, short = 's', default_value_t = false)]
    pub server: bool,

    /// Set the number of tries when connecting to the device to send the executable.
    /// Corresponds to 3dslink's `--retries` argument.
    // Can't use `short = 'r'` because that would conflict with cargo's `--release/-r`
    #[arg(long)]
    pub retries: Option<usize>,

    // Passthrough `cargo build` options.
    #[command(flatten)]
    pub build_args: Build,

    #[arg(from_global)]
    config: Vec<String>,
}

#[derive(Args, Debug)]
pub struct Test {
    /// If set, the built executable will not be sent to the device to run it.
    #[arg(long)]
    pub no_run: bool,

    /// If set, documentation tests will be built instead of unit tests.
    /// This implies `--no-run`, unless Cargo's `target.armv6k-nintendo-3ds.runner`
    /// is configured.
    #[arg(long)]
    pub doc: bool,

    // The test command uses a superset of the same arguments as Run.
    #[command(flatten)]
    pub run_args: Run,
}

#[derive(Args, Debug)]
pub struct New {
    /// Path of the new project.
    #[arg(required = true)]
    pub path: String,

    // The test command uses a superset of the same arguments as Run.
    #[command(flatten)]
    pub cargo_args: RemainingArgs,
}

impl CargoCmd {
    /// Returns the additional arguments run by the "official" cargo subcommand.
    pub(crate) fn cargo_args(&self) -> Vec<String> {
        match self {
            CargoCmd::Build(build) => build.passthrough.cargo_args(),
            CargoCmd::Run(run) => run.build_args.passthrough.cargo_args(),
            CargoCmd::Test(test) => test.cargo_args(),
            CargoCmd::New(new) => {
                // We push the original path in the new command (we captured it in [`New`] to learn about the context)
                let mut cargo_args = new.cargo_args.cargo_args();
                cargo_args.push(new.path.clone());

                cargo_args
            }
            CargoCmd::Passthrough(other) => other.clone().split_off(1),
        }
    }

    /// Returns the cargo subcommand run by `cargo-3ds` when handling a [`CargoCmd`].
    ///
    /// # Notes
    ///
    /// This is not equivalent to the lowercase name of the [`CargoCmd`] variant.
    /// Commands may use different commands under the hood to function (e.g. [`CargoCmd::Run`] uses `build`
    /// if no custom runner is configured).
    pub(crate) fn subcommand_name(&self) -> &str {
        match self {
            CargoCmd::Build(_) => "build",
            CargoCmd::Run(run) => {
                if run.use_custom_runner() {
                    "run"
                } else {
                    "build"
                }
            }
            CargoCmd::Test(_) => "test",
            CargoCmd::New(_) => "new",
            CargoCmd::Passthrough(cmd) => &cmd[0],
        }
    }

    /// Whether or not this command should compile any code, and thus needs import the custom environment configuration (e.g. target spec).
    pub(crate) fn should_compile(&self) -> bool {
        matches!(
            self,
            Self::Build(_) | Self::Run(_) | Self::Test(_) | Self::Passthrough(_)
        )
    }

    /// Whether or not this command should build a 3DSX executable file.
    pub fn should_build_3dsx(&self) -> bool {
        match self {
            Self::Build(_) | CargoCmd::Run(_) => true,
            &Self::Test(Test { doc, .. }) => {
                if doc {
                    eprintln!("Documentation tests requested, no 3dsx will be built");
                    false
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    pub const DEFAULT_MESSAGE_FORMAT: &'static str = "json-render-diagnostics";

    pub fn extract_message_format(&mut self) -> Result<Option<String>, String> {
        let cargo_args = match self {
            Self::Build(build) => &mut build.passthrough.args,
            Self::Run(run) => &mut run.build_args.passthrough.args,
            Self::New(new) => &mut new.cargo_args.args,
            Self::Test(test) => &mut test.run_args.build_args.passthrough.args,
            Self::Passthrough(args) => args,
        };

        let format = Self::extract_message_format_from_args(cargo_args)?;
        if format.is_some() {
            return Ok(format);
        }

        if let Self::Test(Test { doc: true, .. }) = self {
            // We don't care about JSON output for doctests since we're not
            // building any 3dsx etc. Just use the default output as it's more
            // readable compared to DEFAULT_MESSAGE_FORMAT
            Ok(Some(String::from("human")))
        } else {
            Ok(None)
        }
    }

    fn extract_message_format_from_args(
        cargo_args: &mut Vec<String>,
    ) -> Result<Option<String>, String> {
        // Checks for a position within the args where '--message-format' is located
        if let Some(pos) = cargo_args
            .iter()
            .position(|s| s.starts_with("--message-format"))
        {
            // Remove the arg from list so we don't pass anything twice by accident
            let arg = cargo_args.remove(pos);

            // Allows for usage of '--message-format=<format>' and also using space separation.
            // Check for a '=' delimiter and use the second half of the split as the format,
            // otherwise remove next arg which is now at the same position as the original flag.
            let format = if let Some((_, format)) = arg.split_once('=') {
                format.to_string()
            } else {
                // Also need to remove the argument to the --message-format option
                cargo_args.remove(pos)
            };

            // Non-json formats are not supported so the executable exits.
            if format.starts_with("json") {
                Ok(Some(format))
            } else {
                Err(String::from(
                    "error: non-JSON `message-format` is not supported",
                ))
            }
        } else {
            Ok(None)
        }
    }

    /// Runs the custom callback *after* the cargo command, depending on the type of command launched.
    ///
    /// # Examples
    ///
    /// - `cargo 3ds build` and other "build" commands will use their callbacks to build the final `.3dsx` file and link it.
    /// - `cargo 3ds new` and other generic commands will use their callbacks to make 3ds-specific changes to the environment.
    pub fn run_callbacks(&self, messages: &[Message], metadata: Option<&Metadata>) {
        let configs = metadata
            .map(|metadata| self.build_callbacks(messages, metadata))
            .unwrap_or_default();

        let config = match self {
            // If we produced one executable, we will attempt to run that one
            _ if configs.len() == 1 => configs.into_iter().next().unwrap(),

            // --no-run may produce any number of executables, and we skip the callback
            Self::Test(Test { no_run: true, .. }) => return,

            // If using custom runners, they may be able to handle multiple executables,
            // and we also want to skip our own callback. `cargo run` also has its own
            // logic to disallow multiple executables.
            Self::Test(Test { run_args: run, .. }) | Self::Run(run) if run.use_custom_runner() => {
                return
            }

            // Config is ignored by the New callback, using default is fine.
            Self::New(_) => CTRConfig::default(),

            // Otherwise (configs.len() != 1) print an error and exit
            Self::Test(_) | Self::Run(_) => {
                let paths: Vec<_> = configs.into_iter().map(|c| c.path_3dsx()).collect();
                let names: Vec<_> = paths.iter().filter_map(|p| p.file_name()).collect();
                eprintln!(
                    "Error: expected exactly one (1) executable to run, got {}: {names:?}",
                    paths.len(),
                );
                process::exit(1);
            }

            _ => return,
        };

        self.run_callback(&config);
    }

    /// Generate a .3dsx for every executable artifact within the workspace that
    /// was built by the cargo command.
    fn build_callbacks(&self, messages: &[Message], metadata: &Metadata) -> Vec<CTRConfig> {
        let max_artifact_count = metadata.packages.iter().map(|pkg| pkg.targets.len()).sum();
        let mut configs = Vec::with_capacity(max_artifact_count);

        for message in messages {
            let Message::CompilerArtifact(artifact) = message else {
                continue;
            };

            if artifact.executable.is_none()
                || !metadata.workspace_members.contains(&artifact.package_id)
            {
                continue;
            }

            let package = &metadata[&artifact.package_id];
            let config = get_artifact_config(package.clone(), artifact.clone());

            self.build_callback(&config);

            configs.push(config);
        }

        configs
    }

    fn inner_callback(&self) -> Option<&dyn Callbacks> {
        match self {
            Self::Build(cmd) => Some(cmd),
            Self::Run(cmd) => Some(cmd),
            Self::Test(cmd) => Some(cmd),
            Self::New(cmd) => Some(cmd),
            _ => None,
        }
    }
}

impl Callbacks for CargoCmd {
    fn build_callback(&self, config: &CTRConfig) {
        if let Some(cb) = self.inner_callback() {
            cb.build_callback(config);
        }
    }

    fn run_callback(&self, config: &CTRConfig) {
        if let Some(cb) = self.inner_callback() {
            cb.run_callback(config);
        }
    }
}

impl RemainingArgs {
    /// Get the args to be passed to `cargo`.
    pub(crate) fn cargo_args(&self) -> Vec<String> {
        self.split_args().0
    }

    /// Get the args to be passed to the executable itself (not `cargo`).
    pub(crate) fn exe_args(&self) -> Vec<String> {
        self.split_args().1
    }

    fn split_args(&self) -> (Vec<String>, Vec<String>) {
        let mut args = self.args.clone();

        if let Some(split) = args.iter().position(|s| s == "--") {
            let second_half = args.split_off(split + 1);
            // take off the "--" arg we found, we'll add one later if needed
            args.pop();

            (args, second_half)
        } else {
            (args, Vec::new())
        }
    }
}

impl Callbacks for Build {
    /// Callback for `cargo 3ds build`.
    ///
    /// This callback handles building the application as a `.3dsx` file.
    fn build_callback(&self, config: &CTRConfig) {
        eprintln!("Building smdh: {}", config.path_smdh());
        config.build_smdh(self.verbose);

        eprintln!("Building 3dsx: {}", config.path_3dsx());
        build_3dsx(config, self.verbose);
    }
}

impl Callbacks for Run {
    fn build_callback(&self, config: &CTRConfig) {
        self.build_args.build_callback(config);
    }

    /// Callback for `cargo 3ds run`.
    ///
    /// This callback handles launching the application via `3dslink`.
    fn run_callback(&self, config: &CTRConfig) {
        if !self.use_custom_runner() {
            eprintln!("Running 3dslink");
            link(config, self, self.build_args.verbose);
        }
    }
}

impl Run {
    /// Get the args to pass to `3dslink` based on these options.
    pub(crate) fn get_3dslink_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        if let Some(address) = self.address {
            args.extend(["--address".to_string(), address.to_string()]);
        }

        if let Some(argv0) = &self.argv0 {
            args.extend(["--arg0".to_string(), argv0.clone()]);
        }

        if let Some(retries) = self.retries {
            args.extend(["--retries".to_string(), retries.to_string()]);
        }

        if self.server {
            args.push("--server".to_string());
        }

        let exe_args = self.build_args.passthrough.exe_args();
        if !exe_args.is_empty() {
            // For some reason 3dslink seems to want 2 instances of `--`, one
            // in front of all of the args like this...
            args.extend(["--args".to_string(), "--".to_string()]);

            let mut escaped = false;
            for arg in exe_args.iter().cloned() {
                if arg.starts_with('-') && !escaped {
                    // And one before the first `-` arg that is passed in.
                    args.extend(["--".to_string(), arg]);
                    escaped = true;
                } else {
                    args.push(arg);
                }
            }
        }

        args
    }

    /// Returns whether the cargo environment has `target.armv6k-nintendo-3ds.runner`
    /// configured. This will only be checked once during the lifetime of the program,
    /// and takes into account the usual ways Cargo looks for its
    /// [configuration](https://doc.rust-lang.org/cargo/reference/config.html):
    ///
    /// - `.cargo/config.toml`
    /// - Environment variables
    /// - Command-line `--config` overrides
    pub(crate) fn use_custom_runner(&self) -> bool {
        static HAS_RUNNER: OnceLock<bool> = OnceLock::new();

        let &custom_runner_configured = HAS_RUNNER.get_or_init(|| {
            let mut cmd = cargo(&self.config);
            cmd.args([
                // https://github.com/rust-lang/cargo/issues/9301
                "-Z",
                "unstable-options",
                "config",
                "get",
                "target.armv6k-nintendo-3ds.runner",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());

            if self.build_args.verbose {
                print_command(&cmd);
            }

            // `cargo config get` exits zero if the config exists, or nonzero otherwise
            cmd.status().map_or(false, |status| status.success())
        });

        if self.build_args.verbose {
            eprintln!(
                "Custom runner is {}configured",
                if custom_runner_configured { "" } else { "not " }
            );
        }

        custom_runner_configured
    }
}

impl Callbacks for Test {
    fn build_callback(&self, config: &CTRConfig) {
        self.run_args.build_callback(config);
    }

    /// Callback for `cargo 3ds test`.
    ///
    /// This callback handles launching the application via `3dslink`.
    fn run_callback(&self, config: &CTRConfig) {
        if !self.no_run {
            self.run_args.run_callback(config);
        }
    }
}

impl Test {
    fn should_run(&self) -> bool {
        self.run_args.use_custom_runner() && !self.no_run
    }

    /// The args to pass to the underlying `cargo test` command.
    fn cargo_args(&self) -> Vec<String> {
        let mut cargo_args = self.run_args.build_args.passthrough.cargo_args();

        // We can't run 3DS executables on the host, but we want to respect
        // the user's "runner" configuration if set.
        //
        // If doctests were requested, `--no-run` will be rejected on the
        // command line and must be set with RUSTDOCFLAGS instead:
        // https://github.com/rust-lang/rust/issues/87022

        if self.doc {
            cargo_args.extend([
                "--doc".into(),
                // https://github.com/rust-lang/cargo/issues/7040
                "-Z".into(),
                "doctest-xcompile".into(),
            ]);
        } else if !self.should_run() {
            cargo_args.push("--no-run".into());
        }

        cargo_args
    }

    /// Flags to pass to rustdoc via RUSTDOCFLAGS
    pub(crate) fn rustdocflags(&self) -> &'static str {
        if self.should_run() {
            ""
        } else {
            // We don't support running doctests by default, but cargo doesn't like
            // --no-run for doctests, so we have to plumb it in via RUSTDOCFLAGS
            " --no-run"
        }
    }
}

const TOML_CHANGES: &str = r#"ctru-rs = { git = "https://github.com/rust3ds/ctru-rs" }

[package.metadata.cargo-3ds]
romfs_dir = "romfs"
"#;

const CUSTOM_MAIN_RS: &str = r#"use ctru::prelude::*;

fn main() {
    let apt = Apt::new().unwrap();
    let mut hid = Hid::new().unwrap();
    let gfx = Gfx::new().unwrap();
    let _console = Console::new(gfx.top_screen.borrow_mut());

    println!("Hello, World!");
    println!("\x1b[29;16HPress Start to exit");

    while apt.main_loop() {
        gfx.wait_for_vblank();

        hid.scan_input();
        if hid.keys_down().contains(KeyPad::START) {
            break;
        }
    }
}
"#;

impl Callbacks for New {
    /// Callback for `cargo 3ds new`.
    ///
    /// This callback handles the custom environment modifications when creating a new 3DS project.
    fn run_callback(&self, _: &CTRConfig) {
        // Commmit changes to the project only if is meant to be a binary
        if self.cargo_args.args.contains(&"--lib".to_string()) {
            return;
        }

        // Attain a canonicalised path for the new project and it's TOML manifest
        let project_path = fs::canonicalize(&self.path).unwrap();
        let toml_path = project_path.join("Cargo.toml");
        let romfs_path = project_path.join("romfs");
        let main_rs_path = project_path.join("src/main.rs");
        let dummy_romfs_path = romfs_path.join("PUT_YOUR_ROMFS_FILES_HERE.txt");

        // Create the "romfs" directory, and place a dummy file within it.
        fs::create_dir(romfs_path).unwrap();
        fs::File::create(dummy_romfs_path).unwrap();

        // Read the contents of `Cargo.toml` to a string
        let mut buf = String::new();
        fs::File::open(&toml_path)
            .unwrap()
            .read_to_string(&mut buf)
            .unwrap();

        // Add the custom changes to the TOML
        let buf = buf + TOML_CHANGES;
        fs::write(&toml_path, buf).unwrap();

        // Add the custom changes to the main.rs file
        fs::write(main_rs_path, CUSTOM_MAIN_RS).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn verify_app() {
        Cargo::command().debug_assert();
    }

    #[test]
    fn extract_format() {
        const CASES: &[(&[&str], Option<&str>)] = &[
            (&["--foo", "--message-format=json", "bar"], Some("json")),
            (&["--foo", "--message-format", "json", "bar"], Some("json")),
            (
                &[
                    "--foo",
                    "--message-format",
                    "json-render-diagnostics",
                    "bar",
                ],
                Some("json-render-diagnostics"),
            ),
            (
                &["--foo", "--message-format=json-render-diagnostics", "bar"],
                Some("json-render-diagnostics"),
            ),
            (&["--foo", "bar"], None),
        ];

        for (args, expected) in CASES {
            let mut cmd = CargoCmd::Build(Build {
                passthrough: RemainingArgs {
                    args: args.iter().map(ToString::to_string).collect(),
                },
                verbose: false,
            });

            assert_eq!(
                cmd.extract_message_format().unwrap(),
                expected.map(ToString::to_string)
            );

            if let CargoCmd::Build(build) = cmd {
                assert_eq!(build.passthrough.args, vec!["--foo", "bar"]);
            } else {
                unreachable!();
            }
        }
    }

    #[test]
    fn extract_format_err() {
        for args in [&["--message-format=foo"][..], &["--message-format", "foo"]] {
            let mut cmd = CargoCmd::Build(Build {
                passthrough: RemainingArgs {
                    args: args.iter().map(ToString::to_string).collect(),
                },
                verbose: false,
            });

            assert!(cmd.extract_message_format().is_err());
        }
    }

    #[test]
    fn split_run_args() {
        struct TestParam {
            input: &'static [&'static str],
            expected_cargo: &'static [&'static str],
            expected_exe: &'static [&'static str],
        }

        for param in [
            TestParam {
                input: &["--example", "hello-world", "--no-default-features"],
                expected_cargo: &["--example", "hello-world", "--no-default-features"],
                expected_exe: &[],
            },
            TestParam {
                input: &["--example", "hello-world", "--", "--do-stuff", "foo"],
                expected_cargo: &["--example", "hello-world"],
                expected_exe: &["--do-stuff", "foo"],
            },
            TestParam {
                input: &["--lib", "--", "foo"],
                expected_cargo: &["--lib"],
                expected_exe: &["foo"],
            },
            TestParam {
                input: &["foo", "--", "bar"],
                expected_cargo: &["foo"],
                expected_exe: &["bar"],
            },
        ] {
            let input: Vec<&str> = ["cargo", "3ds", "run"]
                .iter()
                .chain(param.input)
                .copied()
                .collect();

            dbg!(&input);
            let Cargo::Input(Input {
                cmd: CargoCmd::Run(Run { build_args, .. }),
                ..
            }) = Cargo::try_parse_from(input).unwrap_or_else(|e| panic!("{e}"))
            else {
                panic!("parsed as something other than `run` subcommand")
            };

            assert_eq!(build_args.passthrough.cargo_args(), param.expected_cargo);
            assert_eq!(build_args.passthrough.exe_args(), param.expected_exe);
        }
    }
}
