# cargo-3ds
Cargo command to work with Nintendo 3DS project binaries. Based on cargo-psp.

# Usage
While you can set the nightly version of Rust as default for the project you're working on (`rustup override nightly`), my suggested method is:
`cargo +nightly 3ds`. \
The commands are the same as cargo ("run" also uses 3dslink, so you can directly use `run` to compile and run on your system).

# Examples: 
`cargo +nightly 3ds build` \
`cargo +nightly 3ds run --release`

You can pass or not `--release` to build with debug symbols or not, and this works for both `build` and `link`.

Any other parameters you pass after the command (being it `build` or `link`) will be passed during the compiling stage to `cargo`.
