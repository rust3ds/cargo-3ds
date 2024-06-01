//! This module defines the in-memory representation of the `cargo-3ds` section
//! of metadata in `Cargo.toml`.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::env;

use cargo_metadata::camino::Utf8PathBuf;
use cargo_metadata::{Artifact, Metadata, PackageId};
use serde::Deserialize;

use crate::CTRConfig;

/// The `cargo-3ds` section of a `Cargo.toml` file for a single package.
#[derive(Debug, Deserialize, Default, PartialEq, Eq)]
pub struct Cargo3DS {
    /// The default configuration for all targets in the package. These values
    /// will be used if a target does not have its own values specified.
    ///
    // It might be nice to use a `#[serde(default)]` attribute here, but it doesn't
    // work with `flatten`: https://github.com/serde-rs/serde/issues/1879
    #[serde(flatten)]
    pub default: TargetMetadata,

    /// Configuration for each example target in the package.
    #[serde(default, rename = "example")]
    pub examples: HashMap<String, TargetMetadata>,

    /// Configuration for each binary target in the package.
    #[serde(default, rename = "bin")]
    pub bins: HashMap<String, TargetMetadata>,

    /// Configuration for each integration test target in the package.
    #[serde(default, rename = "test")]
    pub tests: HashMap<String, TargetMetadata>,

    /// Configuration for the lib test executable (a.k.a unit tests).
    #[serde(default)]
    pub lib: Option<TargetMetadata>,
}

impl Cargo3DS {
    const METADATA_KEY: &'static str = "cargo-3ds";

    /// Collect all `cargo-3ds` [`Metadata`] into a map from package to the
    /// configuration for each package.
    pub fn from_metadata(metadata: &Metadata) -> HashMap<PackageId, Self> {
        let mut result: HashMap<PackageId, Self> = HashMap::default();

        // TODO: we ignore top-level [workspace.metadata."cargo-3ds"] for now, but we could
        // use it to set defaults for the entire workspace, or something. It would make
        // paths a little more confusing and require different default handling probably.

        for package in &metadata.packages {
            let package_config = result.entry(package.id.clone()).or_default();

            if package.description.is_some() {
                package_config
                    .default
                    .description
                    .clone_from(&package.description);
            }

            // TODO copy authors. Maybe we should do a ", " join of all authors?

            if let Some(package_meta) =
                package
                    .metadata
                    .get(Self::METADATA_KEY)
                    .and_then(|workspace_meta| {
                        serde_json::from_value::<Cargo3DS>(workspace_meta.clone()).ok()
                    })
            {
                package_config.merge(package_meta);
            }
        }

        result
    }

    /// Walk the list of provided messages and return a [`CTRConfig`] for each
    /// executable artifact that was built (e.g. an example, a test, or the lib tests).
    pub fn artifact_config(&self, metadata: &Metadata, artifact: &Artifact) -> Option<CTRConfig> {
        let package = &metadata[&artifact.package_id];
        let target = &artifact.target;
        let profile = &artifact.profile;
        let mut target_name = target.name.clone();

        let mut metadata = None;
        for kind in &target.kind {
            metadata = match kind.as_str() {
                "lib" | "rlib" | "dylib" | "bin" if profile.test => {
                    target_name = format!("{target_name} tests");
                    self.lib.as_ref()
                }
                "example" => {
                    target_name = format!("{target_name} - {} example", package.name);
                    self.examples.get(&target_name)
                }
                "test" => self.tests.get(&target_name),
                "bin" => self.bins.get(&target_name),
                _ => continue,
            };

            break;
        }

        let target_metadata = metadata.unwrap_or(&self.default);

        // TODO: restore old behavior of trying ./icon.png if it exists
        let icon_path = target_metadata
            .icon
            .as_ref()
            .and_then(|path| Some(package.manifest_path.parent()?.join(path)))
            .unwrap_or_else(|| {
                let devkitpro_dir = Utf8PathBuf::from(&env::var("DEVKITPRO").unwrap());
                devkitpro_dir.join("libctru").join("default_icon.png")
            });

        let author = target_metadata
            .author
            .clone()
            .unwrap_or_else(|| String::from("Unspecified Author"));

        let description = target_metadata
            .description
            .clone()
            .unwrap_or_else(|| String::from("Homebrew Application"));

        let executable = artifact.executable.clone()?;

        Some(CTRConfig {
            name: target_name,
            author,
            description,
            icon: icon_path.into(),
            target_path: executable.into(),
            cargo_manifest_path: package.manifest_path.clone().into(),
        })
    }

    /// Merge another [`Cargo3DS`] into this one. Each target specified by the given
    /// configuration may set options to overide the current configuration.
    fn merge(&mut self, other: Self) {
        self.default.merge(other.default);

        for (self_targets, other_targets) in [
            (&mut self.bins, other.bins),
            (&mut self.examples, other.examples),
            (&mut self.tests, other.tests),
        ] {
            for (name, target) in other_targets {
                match self_targets.entry(name) {
                    Entry::Occupied(mut t) => t.get_mut().merge(target),
                    Entry::Vacant(t) => {
                        t.insert(target);
                    }
                }
            }
        }

        self.lib = match (self.lib.take(), other.lib) {
            (Some(mut lib_meta), Some(other_meta)) => {
                lib_meta.merge(other_meta);
                Some(lib_meta)
            }
            (lib, other_lib) => lib.or(other_lib),
        };
    }
}

// TODO: maybe this should just *be* CTRConfig? It might not be necessary to do the
// translation between them if we just deserialize directly into CTRConfig.
#[derive(Default, Debug, Deserialize, PartialEq, Eq)]
pub struct TargetMetadata {
    /// The path to the icon file for the executable, relative to `Cargo.toml`.
    pub icon: Option<Utf8PathBuf>,

    /// The path to the ROMFS directory for the executable, relative to `Cargo.toml`.
    #[serde(alias = "romfs-dir")]
    pub romfs_dir: Option<Utf8PathBuf>,

    /// A short description of the executable, used in the homebrew menu.
    pub description: Option<String>,

    /// The author of the executable, used in the homebrew menu.
    pub author: Option<String>,
}

impl TargetMetadata {
    fn merge(&mut self, other: Self) {
        self.icon = other.icon.or(self.icon.take());
        self.romfs_dir = other.romfs_dir.or(self.romfs_dir.take());
    }
}

#[cfg(test)]
mod tests {
    use toml::toml;

    use super::*;

    #[test]
    fn from_toml() {
        let value = toml! {
            romfs_dir = "my_romfs"

            example.example1.icon = "example1.png"
            example.example1.romfs-dir = "example1-romfs"

            example.example2.icon = "example2.png"

            test.test1.romfs_dir = "test1-romfs"

            lib.icon = "lib.png"
            lib.romfs_dir = "lib-romfs"
        };

        let config: Cargo3DS = value.try_into().unwrap();

        assert_eq!(
            config.default,
            TargetMetadata {
                romfs_dir: Some(Utf8PathBuf::from("my_romfs")),
                icon: None,
                ..Default::default()
            }
        );

        assert_eq!(
            config.examples,
            HashMap::from_iter([
                (
                    String::from("example1"),
                    TargetMetadata {
                        icon: Some(Utf8PathBuf::from("example1.png")),
                        romfs_dir: Some(Utf8PathBuf::from("example1-romfs")),
                        ..Default::default()
                    }
                ),
                (
                    String::from("example2"),
                    TargetMetadata {
                        icon: Some(Utf8PathBuf::from("example2.png")),
                        ..Default::default()
                    }
                ),
            ])
        );

        assert_eq!(
            config.tests,
            HashMap::from_iter([(
                String::from("test1"),
                TargetMetadata {
                    romfs_dir: Some(Utf8PathBuf::from("test1-romfs")),
                    ..Default::default()
                }
            )])
        );

        assert_eq!(
            config.lib,
            Some(TargetMetadata {
                icon: Some(Utf8PathBuf::from("lib.png")),
                romfs_dir: Some(Utf8PathBuf::from("lib-romfs")),
                ..Default::default()
            })
        );
    }

    #[test]
    fn from_cargo_metadata() {
        // This is a real metadata object from ctru-rs workspace. Ideally we'd
        // have a bunch more keys configured, but this at least tests the basic
        // collection / conversion process.
        let metadata: Metadata =
            serde_json::from_str(include_str!("./test_metadata.json")).unwrap();

        let config = Cargo3DS::from_metadata(&metadata);
        assert_eq!(config.len(), 4);

        // Assert that at least one package has the configured value for its romfs_dir.
        config
            .values()
            .find(|cfg| cfg.default.romfs_dir == Some(Utf8PathBuf::from("examples/romfs")))
            .unwrap();
    }

    #[test]
    fn merge_defaults() {
        let mut config = Cargo3DS::default();

        let first: Cargo3DS = toml! {
            romfs_dir = "first_romfs"

            bin.cool-bin.icon = "cool.png"

            example.example1.icon = "example1.png"
            example.example2.icon = "example2.png"

            test.test1.romfs_dir = "test1-romfs"

            lib.romfs_dir = "lib-romfs"
        }
        .try_into()
        .unwrap();

        let next: Cargo3DS = toml! {
            example.example1.romfs-dir = "example-dir"
            test.test1.romfs_dir = "test1-next-romfs"
            lib.romfs_dir = "lib-override"
        }
        .try_into()
        .unwrap();

        config.merge(first);
        config.merge(next);

        let expected: Cargo3DS = toml! {
            romfs_dir = "first_romfs"

            bin.cool-bin.icon = "cool.png"

            example.example1.icon = "example1.png"
            example.example1.romfs-dir = "example-dir"

            example.example2.icon = "example2.png"

            test.test1.romfs_dir = "test1-next-romfs"

            lib.romfs_dir = "lib-override"
        }
        .try_into()
        .unwrap();

        assert_eq!(config, expected);
    }
}
