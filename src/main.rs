use cargo_3ds::commands::Cargo;
use cargo_3ds::{
    build_3dsx, build_elf, build_smdh, check_rust_version, get_message_format, get_metadata,
    get_should_link, link,
};
use clap::Parser;
use std::process;

fn main() {
    check_rust_version();

    let Cargo::Input(mut input) = Cargo::parse();

    let should_link = get_should_link(&mut input);
    let message_format = get_message_format(&mut input);

    let (status, messages) = build_elf(input.cmd, &message_format, &input.cargo_opts);

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    if !input.cmd.should_build_3dsx() {
        return;
    }

    eprintln!("Getting metadata");
    let app_conf = get_metadata(&messages);

    eprintln!("Building smdh:{}", app_conf.path_smdh().display());
    build_smdh(&app_conf);

    eprintln!("Building 3dsx: {}", app_conf.path_3dsx().display());
    build_3dsx(&app_conf);

    if should_link {
        eprintln!("Running 3dslink");
        link(&app_conf);
    }
}
