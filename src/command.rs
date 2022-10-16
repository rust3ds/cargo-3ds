// TODO: docstrings for everything!!!

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
pub enum Cargo {
    #[command(name = "3ds")]
    Input(Input),
}

#[derive(Args, Debug)]
#[command(version, about)]
pub struct Input {
    /// The cargo command to run. This command will be forwarded to the real
    /// `cargo` with the appropriate arguments for a 3DS executable.
    #[command(subcommand)]
    pub cmd: CargoCommand,

    /// Don't actually run any commands, just echo them to the console.
    /// This is mostly intended for testing.
    #[arg(long, hide = true)]
    pub dry_run: bool,

    /// Pass additional options through to the `cargo` command.
    ///
    /// To pass flags that start with `-`, you must use `--` to separate `cargo 3ds`
    /// options from cargo options. All args after `--` will be passed through
    /// to cargo unmodified.
    ///
    /// If one of the arguments is itself `--`, the args following that will be
    /// considered as args to pass to the executable, rather than to `cargo`
    /// (only works for the `run` or `test` commands). For example, if you want
    /// to pass some args to the executable directly it might look like this:
    ///
    /// > cargo 3ds run -- -- arg1 arg2
    #[arg(trailing_var_arg = true)]
    #[arg(global = true)]
    cargo_options: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum CargoCommand {
    /// Builds an executable suitable to run on a 3DS (3dsx).
    Build,
    /// Equivalent to `cargo check`.
    Check,
    /// Equivalent to `cargo clippy`.
    Clippy,
    /// Equivalent to `cargo doc`.
    Doc,
    /// Builds an executable and sends it to a device with `3dslink`.
    Run(Run),
    /// Builds a test executable and sends it to a device with `3dslink`.
    ///
    /// This can be used with `--test` for integration tests, or `--lib` for
    /// unit tests (which require a custom test runner).
    Test(Test),
    //
    // TODO: this doesn't seem to work for some reason...
    // #[command(external_subcommand)]
    // Other(Vec<String>),
}

#[derive(Args, Debug)]
pub struct Test {
    /// If set, the built executable will not be sent to the device to run it.
    #[arg(long)]
    pub no_run: bool,
    #[command(flatten)]
    pub run_args: Run,
}

#[derive(Args, Debug)]
pub struct Run {
    /// Specify the IP address of the device to send the executable to.
    ///
    /// Corresponds to 3dslink's `--address` arg, which defaults to automatically
    /// finding the device.
    #[arg(long, short = 'a')]
    pub address: Option<String>,

    /// Set the 0th argument of the executable when running it. Corresponds to
    /// 3dslink's `--argv0` argument.
    #[arg(long, short = '0')]
    pub argv0: Option<String>,

    /// Start the 3dslink server after sending the executable. Corresponds to
    /// 3dslink's `--server` argument.
    #[arg(long, short = 's')]
    pub server: bool,

    /// Set the number of tries when connecting to the device to send the executable.
    /// Corresponds to 3dslink's `--retries` argument.
    // Can't use `short = 'r'` because that would conflict with cargo's `--release/-r`
    #[arg(long)]
    pub retries: Option<usize>,
}

impl Input {
    /// Get the args to be passed to the executable itself (not `cargo`).
    pub fn cargo_opts(&self) -> &[String] {
        self.split_args().0
    }

    /// Get the args to be passed to the executable itself (not `cargo`).
    pub fn exe_args(&self) -> &[String] {
        self.split_args().1
    }

    fn split_args(&self) -> (&[String], &[String]) {
        if let Some(split) = self.cargo_options.iter().position(|arg| arg == "--") {
            let split = if &self.cargo_options[split] == "--" {
                split + 1
            } else {
                split
            };
            self.cargo_options.split_at(split)
        } else {
            (&self.cargo_options[..], &[])
        }
    }
}
