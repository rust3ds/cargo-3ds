use clap::{ArgEnum, Parser};
use std::fmt::{Display, Formatter};

#[derive(Parser)]
pub struct Input {
    #[clap(arg_enum)]
    pub cmd: CargoCommand,
    pub cargo_opts: Vec<String>,
}

#[derive(ArgEnum, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
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
            CargoCommand::Build => write!(f, "build"),
            CargoCommand::Run => write!(f, "run"),
            CargoCommand::Test => write!(f, "test"),
            CargoCommand::Check => write!(f, "check"),
            CargoCommand::Clippy => write!(f, "clippy"),
        }
    }
}
