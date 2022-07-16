use cargo_3ds::commands::{CargoCommand, Input};
use cargo_3ds::{build_3dsx, build_elf, build_smdh, check_rust_version, get_metadata, link};
use clap::Parser;
use std::process;

fn main() {
    check_rust_version();

    let mut input: Input = Input::parse();

    let should_link = input.cmd == CargoCommand::Build
        || (input.cmd == CargoCommand::Test
            && if input.cargo_opts.contains(&"--no-run".to_string()) {
                false
            } else {
                input.cargo_opts.push("--no-run".to_string());
                true
            });

    let message_format = if let Some(pos) = input
        .cargo_opts
        .iter()
        .position(|s| s.starts_with("--message-format"))
    {
        input.cargo_opts.remove(pos);
        let format = if let Some((_, format)) = input
            .cargo_opts
            .get(pos)
            .unwrap()
            .to_string()
            .split_once('=')
        {
            format.to_string()
        } else {
            input.cargo_opts.remove(pos).to_string()
        };
        if !format.starts_with("json") {
            eprintln!("error: non-JSON `message-format` is not supported");
            process::exit(1);
        } else {
            format
        }
    } else {
        "json-render-diagnostics".to_string()
    };

    let (status, messages) = build_elf(input.cmd, &message_format, &input.cargo_opts);

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    if !input.cmd.should_build_3dsx() {
        return;
    }

    println!("Getting metadata");
    let app_conf = get_metadata(&messages);

    println!("Building smdh:{}", app_conf.path_smdh().display());
    build_smdh(&app_conf);

    println!("Building 3dsx: {}", app_conf.path_3dsx().display());
    build_3dsx(&app_conf);

    if should_link {
        println!("Running 3dslink");
        link(&app_conf);
    }
}
