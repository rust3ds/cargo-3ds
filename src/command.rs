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
    Build(CargoArgs),

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
pub struct CargoArgs {
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
    pub cargo_args: CargoArgs,
}

impl CargoArgs {
    /// Get the args to be passed to the executable itself (not `cargo`).
    pub fn cargo_opts(&self) -> &[String] {
        self.split_args().0
    }

    /// Get the args to be passed to the executable itself (not `cargo`).
    pub fn exe_args(&self) -> &[String] {
        self.split_args().1
    }

    fn split_args(&self) -> (&[String], &[String]) {
        if let Some(split) = self
            .args
            .iter()
            .position(|s| s == "--" || !s.starts_with('-'))
        {
            let split = if self.args[split] == "--" {
                split + 1
            } else {
                split
            };
            self.args.split_at(split)
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
}
