use std::process;

use cargo_3ds::command::Cargo;
use cargo_3ds::{check_rust_version, run_cargo};
use clap::Parser;

fn main() {
    let Cargo::Input(mut input) = Cargo::parse();

    // Depending on the command, we might have different base requirements for the Rust version.
    check_rust_version(&input);

    let message_format = match input.cmd.extract_message_format() {
        Ok(fmt) => fmt,
        Err(msg) => {
            eprintln!("{msg}");
            process::exit(1)
        }
    };

    let (status, messages) = run_cargo(&input, message_format);

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    input.cmd.run_callback(&messages);
}
