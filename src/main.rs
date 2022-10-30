use cargo_3ds::command::{Cargo, CargoCmd, Input, Run, Test};
use cargo_3ds::{
    build_3dsx, build_elf, build_smdh, check_rust_version, get_message_format, get_metadata,
    get_should_link, link,
};
use clap::{CommandFactory, FromArgMatches, Parser};
use std::process;

fn main() {
    check_rust_version();

    let Cargo::Input(mut input) = Cargo::parse();

    dbg!(&input);

    let cargo_args = match &input.cmd {
        CargoCmd::Build(cargo_args)
        | CargoCmd::Run(Run { cargo_args, .. })
        | CargoCmd::Test(Test {
            run_args: Run { cargo_args, .. },
            ..
        }) => cargo_args,
        CargoCmd::Passthrough(other) => todo!(),
    };

    dbg!(cargo_args.cargo_opts());
    dbg!(cargo_args.exe_args());

    // let
    // let message_format = get_message_format(&mut input);

    // let (status, messages) = build_elf(input.cmd, &message_format, &input.cargo_opts);

    // if !status.success() {
    //     process::exit(status.code().unwrap_or(1));
    // }

    // if !input.cmd.should_build_3dsx() {
    //     return;
    // }

    // eprintln!("Getting metadata");
    // let app_conf = get_metadata(&messages);

    // eprintln!("Building smdh:{}", app_conf.path_smdh().display());
    // build_smdh(&app_conf);

    // eprintln!("Building 3dsx: {}", app_conf.path_3dsx().display());
    // build_3dsx(&app_conf);

    // if should_link {
    //     eprintln!("Running 3dslink");
    //     link(&app_conf);
    // }
}
