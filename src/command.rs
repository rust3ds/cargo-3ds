use clap::{AppSettings, Args, Parser, ValueEnum};

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
