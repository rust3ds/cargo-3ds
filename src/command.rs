use clap::{AppSettings, Args, Parser, ValueEnum};
use std::fmt::{Display, Formatter};

#[derive(Parser)]
#[clap(name = "cargo")]
#[clap(bin_name = "cargo")]
pub enum Cargo {
    #[clap(name = "3ds")]
    Input(Input),
}

#[derive(Args)]
#[clap(about)]
#[clap(global_setting(AppSettings::AllowLeadingHyphen))]
pub struct Input {
    #[clap(value_enum)]
    pub cmd: CargoCommand,
    pub cargo_opts: Vec<String>,
}

#[derive(ValueEnum, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum CargoCommand {
    Build,
    Run,
    Test,
    Check,
    Clippy,
}

impl CargoCommand {
    pub fn should_build_3dsx(&self) -> bool {
        matches!(
            self,
            CargoCommand::Build | CargoCommand::Run | CargoCommand::Test
        )
    }
}

impl Display for CargoCommand {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CargoCommand::Build | CargoCommand::Run => write!(f, "build"),
            CargoCommand::Test => write!(f, "test"),
            CargoCommand::Check => write!(f, "check"),
            CargoCommand::Clippy => write!(f, "clippy"),
        }
    }
}