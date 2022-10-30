use cargo_3ds::command::Cargo;
use cargo_3ds::{build_3dsx, build_smdh, check_rust_version, get_metadata, link, run_cargo};

use clap::Parser;

use std::process;

fn main() {
    check_rust_version();

    let Cargo::Input(mut input) = Cargo::parse();

    let message_format = match input.cmd.extract_message_format() {
        Ok(fmt) => fmt,
        Err(msg) => {
            eprintln!("{msg}");
            process::exit(1)
        }
    };

    let (status, messages) = run_cargo(&input.cmd, message_format);

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

    if input.cmd.should_link_to_device() {
        // TODO plumb in exe_args and various 3dslink args

        eprintln!("Running 3dslink");
        link(&app_conf);
    }
}
