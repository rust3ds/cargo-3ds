# cargo-3ds

Cargo command to work with Nintendo 3DS project binaries. Based on cargo-psp.

## Usage

Use the nightly toolchain to build 3DS apps (either by using `rustup override nightly` for the project directory or by adding `+nightly` in the `cargo` invocation).

```txt
Commands:
  build
          Builds an executable suitable to run on a 3DS (3dsx)
  run
          Builds an executable and sends it to a device with `3dslink`
  test
          Builds a test executable and sends it to a device with `3dslink`
  help
          Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help information (use `-h` for a summary)

  -V, --version
          Print version information
```

Additional arguments will be passed through to the given subcommand.
See [passthrough arguments](#passthrough-arguments) for more details.

It is also possible to pass any other `cargo` command (e.g. `doc`, `check`),
and all its arguments will be passed through directly to `cargo` unmodified,
with the proper `RUSTFLAGS` and `--target` set for the 3DS target.

### Basic Examples

* `cargo 3ds build`
* `cargo 3ds check --verbose`
* `cargo 3ds run --release --example foo`
* `cargo 3ds test --no-run`

### Running executables

`cargo 3ds test` and `cargo 3ds run` use the `3dslink` tool to send built
executables to a device, and thus accept specific related arguments that correspond
to `3dslink` arguments:

```txt
-a, --address <ADDRESS>
      Specify the IP address of the device to send the executable to.

      Corresponds to 3dslink's `--address` arg, which defaults to automatically finding the device.

-0, --argv0 <ARGV0>
      Set the 0th argument of the executable when running it. Corresponds to 3dslink's `--argv0` argument

-s, --server
      Start the 3dslink server after sending the executable. Corresponds to 3dslink's `--server` argument

  --retries <RETRIES>
      Set the number of tries when connecting to the device to send the executable. Corresponds to 3dslink's `--retries` argument
```

### Passthrough Arguments

Due to the way `cargo-3ds`, `cargo`, and `3dslink` parse arguments, there is
unfortunately some complexity required when invoking an executable with arguments.

All unrecognized arguments beginning with a dash (e.g. `--release`, `--features`,
etc.) will be passed through to `cargo` directly.

> **NOTE:** arguments for [running executables](#running-executables) must be
> specified *before* other unrecognized `cargo` arguments. Otherwise they will
> be treated as passthrough arguments which will most likely fail the resulting
> `cargo` command.

An optional `--` may be used to explicitly pass subsequent args to `cargo`, including
arguments to pass to the executable itself. To separate `cargo` arguments from
executable arguments, *another* `--` can be used. For example:

* `cargo 3ds run -- -- xyz`

    Builds an executable and send it to a device to run it with the argument `xyz`.

* `cargo 3ds test --address 192.168.0.2 -- -- --test-arg 1`

  Builds a test executable and attempts to send it to a device with the
  address `192.168.0.2` and run it using the arguments `["--test-arg", "1"]`.

* `cargo 3ds test --address 192.168.0.2 --verbose -- --test-arg 1`

  Build a test executable with `cargo test --verbose`, and attempts to send
  it to a device with the address `192.168.0.2` and run it using the arguments
  `["--test-arg", "1"]`.

  This works without two `--` instances because `--verbose` begins the set of
  `cargo` arguments and ends the set of 3DS-specific arguments.

### Caveats

Due to the fact that only one executable at a time can be sent with `3dslink`,
by default only the "last" executable built will be used. If a `test` or `run`
command builds more than one binary, you may need to filter it in order to run
the executable you want.

Doc tests sort of work, but `cargo-3ds` uses a number of unstable cargo and
rustdoc features to make them work, so the output won't be as pretty and will
require some manual workarounds to actually run the tests and see output from them.
For now, `cargo 3ds test --doc` will not build a 3dsx file or use `3dslink` at all.

## License

This project is distributed under the MIT license or the Apache-2.0 license.
