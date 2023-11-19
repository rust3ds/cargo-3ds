use std::error::Error;
use std::io::Read;
use std::process::{Command, Stdio};

use cargo_metadata::Target;
use serde::Deserialize;

use crate::print_command;

/// In lieu of <https://github.com/oli-obk/cargo_metadata/issues/107>
/// and to avoid pulling in the real `cargo`
/// [data structures](https://docs.rs/cargo/latest/cargo/core/compiler/unit_graph/type.UnitGraph.html)
/// as a dependency, we define the subset of the build graph we care about.
#[derive(Deserialize)]
pub struct UnitGraph {
    pub version: i32,
    pub units: Vec<Unit>,
}

impl UnitGraph {
    /// Collect the unit graph via Cargo's `--unit-graph` flag.
    /// This runs the same command as the actual build, except nothing is actually
    /// build and the graph is output instead.
    ///
    /// See <https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#unit-graph>.
    pub fn from_cargo(cargo_cmd: &Command, verbose: bool) -> Result<Self, Box<dyn Error>> {
        // Since Command isn't Clone, copy it "by hand", by copying its args and envs
        let mut cmd = Command::new(cargo_cmd.get_program());

        let mut args = cargo_cmd.get_args();
        cmd.args(args.next())
            // These options must be added before any possible `--`, so the best
            // place is to just stick them immediately after the first arg (subcommand)
            .args(["-Z", "unstable-options", "--unit-graph"])
            .args(args)
            .envs(cargo_cmd.get_envs().filter_map(|(k, v)| Some((k, v?))))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if verbose {
            print_command(&cmd);
        }

        let mut proc = cmd.spawn()?;
        let stdout = proc.stdout.take().unwrap();
        let mut stderr = proc.stderr.take().unwrap();

        let result: Self = serde_json::from_reader(stdout).map_err(|err| {
            let mut stderr_str = String::new();
            let _ = stderr.read_to_string(&mut stderr_str);

            let _ = proc.wait();
            format!("unable to parse `--unit-graph` json: {err}\nstderr: `{stderr_str}`")
        })?;

        let _status = proc.wait()?;
        // TODO:
        // `cargo run --unit-graph` panics at src/cargo/ops/cargo_run.rs:83:5
        // I should probably file a bug for that, then return the error here when it's fixed,
        // but for now just ignore it since we still get valid JSON from the command.
        //
        // if !status.success() {
        //     return Err(format!("`cargo --unit-graph` exited with status {status:?}").into());
        // }

        if result.version == 1 {
            Ok(result)
        } else {
            Err(format!(
                "unknown `cargo --unit-graph` output version {}",
                result.version
            ))?
        }
    }
}

#[derive(Deserialize)]
pub struct Unit {
    pub target: Target,
    pub profile: Profile,
}

/// This struct is very similar to [`cargo_metadata::ArtifactProfile`], but seems
/// to have some slight differences so we define a different version. We only
/// really care about `debuginfo` anyway.
#[derive(Deserialize)]
pub struct Profile {
    pub debuginfo: Option<u32>,
}
