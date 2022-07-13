# cargo-3ds
Cargo command to work with Nintendo 3DS project binaries. Based on cargo-psp.

# Usage
Use the nightly toolchain to build 3DS apps (either by using `rustup override nightly` for the project directory or by adding `+nightly` in the `cargo` invocation).

Available commands:
```
    build           build a 3dsx executable.
    run             build a 3dsx executable and send it to a device with 3dslink.
    test            build a 3dsx executable from unit/integration tests and send it to a device.
    <cargo-command> execute some other Cargo command with 3ds options configured (ex. check or clippy).
```
    
Additional arguments will be passed through to `<cargo-command>`. Some that are supported include:
```
    [build | run | test] --release
    test --no-run
```
    
Other flags and commands may work, but haven't been tested.

# Examples
`cargo 3ds build` \
`cargo 3ds run --release` \
`cargo 3ds test --no-run`
