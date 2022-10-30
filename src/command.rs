use clap::{Args, Parser, Subcommand};

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
}

/// The cargo command to run. This command will be forwarded to the real
/// `cargo` with the appropriate arguments for a 3DS executable.
///
/// If another command is passed which is not recognized, it will be passed
/// through unmodified to `cargo` with RUSTFLAGS set for the 3DS.
#[derive(Subcommand, Debug)]
#[command(allow_external_subcommands = true)]
pub enum CargoCmd {
    /// Builds an executable suitable to run on a 3DS (3dsx).
    Build(RemainingArgs),

    /// Builds an executable and sends it to a device with `3dslink`.
    Run(Run),

    /// Builds a test executable and sends it to a device with `3dslink`.
    ///
    /// This can be used with `--test` for integration tests, or `--lib` for
    /// unit tests (which require a custom test runner).
    Test(Test),

    // NOTE: it seems docstring + name for external subcommands are not rendered
    // in help, but we might as well set them here in case a future version of clap
    // does include them in help text.
    /// Run any other `cargo` command with RUSTFLAGS set for the 3DS.
    #[command(external_subcommand, name = "COMMAND")]
    Passthrough(Vec<String>),
}

#[derive(Args, Debug)]
pub struct RemainingArgs {
    /// Pass additional options through to the `cargo` command.
    ///
    /// To pass flags that start with `-`, you must use `--` to separate `cargo 3ds`
    /// options from cargo options. Any argument after `--` will be passed through
    /// to cargo unmodified.
    ///
    /// If one of the arguments is itself `--`, the args following that will be
    /// considered as args to pass to the executable, rather than to `cargo`
    /// (only works for the `run` or `test` commands). For example, if you want
    /// to pass some args to the executable directly it might look like this:
    ///
    /// > cargo 3ds run -- -- arg1 arg2
    #[arg(trailing_var_arg = true)]
    #[arg(allow_hyphen_values = true)]
    #[arg(global = true)]
    #[arg(name = "CARGO_ARGS")]
    args: Vec<String>,
}

#[derive(Parser, Debug)]
pub struct Test {
    /// If set, the built executable will not be sent to the device to run it.
    #[arg(long)]
    pub no_run: bool,

    // The test command uses a superset of the same arguments as Run.
    #[command(flatten)]
    pub run_args: Run,
}

#[derive(Parser, Debug)]
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

    // Passthrough cargo options.
    #[command(flatten)]
    pub cargo_args: RemainingArgs,
}

impl CargoCmd {
    /// Whether or not this command should build a 3DSX executable file.
    pub fn should_build_3dsx(&self) -> bool {
        matches!(self, Self::Build(_) | Self::Run(_) | Self::Test(_))
    }

    /// Whether or not the resulting executable should be sent to the 3DS with
    /// `3dslink`.
    pub fn should_link_to_device(&self) -> bool {
        match self {
            CargoCmd::Test(test) => !test.no_run,
            CargoCmd::Run(_) => true,
            _ => false,
        }
    }

    pub const DEFAULT_MESSAGE_FORMAT: &str = "json-render-diagnostics";

    pub fn extract_message_format(&mut self) -> Result<Option<String>, String> {
        Self::extract_message_format_from_args(match self {
            CargoCmd::Build(args) => &mut args.args,
            CargoCmd::Run(run) => &mut run.cargo_args.args,
            CargoCmd::Test(test) => &mut test.run_args.cargo_args.args,
            CargoCmd::Passthrough(args) => args,
        })
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
}

impl RemainingArgs {
    /// Get the args to be passed to the executable itself (not `cargo`).
    pub fn cargo_args(&self) -> &[String] {
        self.split_args().0
    }

    /// Get the args to be passed to the executable itself (not `cargo`).
    pub fn exe_args(&self) -> &[String] {
        self.split_args().1
    }

    fn split_args(&self) -> (&[String], &[String]) {
        if let Some(split) = self.args.iter().position(|s| s == "--") {
            self.args.split_at(split + 1)
        } else {
            (&self.args[..], &[])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::CommandFactory;

    #[test]
    fn verify_app() {
        Cargo::command().debug_assert();
    }

    #[test]
    fn extract_format() {
        for (args, expected) in [
            (&["--foo", "--message-format=json", "bar"][..], Some("json")),
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
        ] {
            let mut cmd = CargoCmd::Build(RemainingArgs {
                args: args.iter().map(ToString::to_string).collect(),
            });

            assert_eq!(
                cmd.extract_message_format().unwrap(),
                expected.map(ToString::to_string)
            );

            if let CargoCmd::Build(args) = cmd {
                assert_eq!(args.args, vec!["--foo", "bar"]);
            } else {
                unreachable!();
            }
        }
    }

    #[test]
    fn extract_format_err() {
        for args in [&["--message-format=foo"][..], &["--message-format", "foo"]] {
            let mut cmd = CargoCmd::Build(RemainingArgs {
                args: args.iter().map(ToString::to_string).collect(),
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
                expected_cargo: &["--example", "hello-world", "--"],
                expected_exe: &["--do-stuff", "foo"],
            },
            TestParam {
                input: &["--lib", "--", "foo"],
                expected_cargo: &["--lib", "--"],
                expected_exe: &["foo"],
            },
            TestParam {
                input: &["foo", "--", "bar"],
                expected_cargo: &["foo", "--"],
                expected_exe: &["bar"],
            },
        ] {
            let Run { cargo_args, .. } =
                Run::parse_from(std::iter::once(&"run").chain(param.input));

            assert_eq!(cargo_args.cargo_args(), param.expected_cargo);
            assert_eq!(cargo_args.exe_args(), param.expected_exe);
        }
    }
}
