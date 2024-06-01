// Beginning sketch of

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::PathBuf;

use cargo_metadata::{Metadata, PackageId};
use serde::Deserialize;

/// The `cargo-3ds` section of a `Cargo.toml` file for a single package.
#[derive(Debug, Deserialize, Default, PartialEq, Eq)]
pub struct Cargo3DS {
    /// The default configuration for all targets in the package. These values
    /// will be used if a target does not have its own values specified.
    // It might be nice to use a `#[serde(default)]` attribute here, but it doesn't
    // work with `flatten`: https://github.com/serde-rs/serde/issues/1879
    #[serde(flatten)]
    pub default: TargetMetadata,

    /// Configuration for each example target in the package.
    #[serde(default)]
    pub examples: HashMap<String, TargetMetadata>,

    /// Configuration for each integration test target in the package.
    #[serde(default)]
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

    /// Merge another [`Cargo3DS`] into this one. Each target specified by the given
    /// configuration may set options to overide the current configuration.
    fn merge(&mut self, other: Self) {
        self.default.merge(other.default);

        for (name, example) in other.examples {
            match self.examples.entry(name) {
                Entry::Occupied(mut t) => t.get_mut().merge(example),
                Entry::Vacant(t) => {
                    t.insert(example);
                }
            }
        }

        for (name, test) in other.tests {
            match self.tests.entry(name) {
                Entry::Occupied(mut t) => t.get_mut().merge(test),
                Entry::Vacant(t) => {
                    t.insert(test);
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

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct TargetMetadata {
    /// The path to the icon file for the target, relative to `Cargo.toml`.
    pub icon: Option<PathBuf>,

    /// The path to the ROMFS directory for the target, relative to `Cargo.toml`.
    #[serde(alias = "romfs-dir")]
    pub romfs_dir: Option<PathBuf>,

    /// A short description of the target, used in the homebrew menu.
    pub description: Option<String>,
}

impl TargetMetadata {
    fn merge(&mut self, other: Self) {
        self.icon = other.icon.or(self.icon.take());
        self.romfs_dir = other.romfs_dir.or(self.romfs_dir.take());
    }
}

impl Default for TargetMetadata {
    fn default() -> Self {
        Self {
            icon: Some(PathBuf::from("icon.png")),
            romfs_dir: Some(PathBuf::from("romfs")),
            description: None,
        }
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

            examples.example1.icon = "example1.png"
            examples.example1.romfs-dir = "example1-romfs"

            examples.example2.icon = "example2.png"

            tests.test1.romfs_dir = "test1-romfs"

            lib.icon = "lib.png"
            lib.romfs_dir = "lib-romfs"
        };

        let config: Cargo3DS = value.try_into().unwrap();

        assert_eq!(
            config.default,
            TargetMetadata {
                icon: None,
                romfs_dir: Some(PathBuf::from("my_romfs")),
                description: None,
            }
        );

        assert_eq!(
            config.examples,
            HashMap::from_iter([
                (
                    String::from("example1"),
                    TargetMetadata {
                        icon: Some(PathBuf::from("example1.png")),
                        romfs_dir: Some(PathBuf::from("example1-romfs")),
                        description: None,
                    }
                ),
                (
                    String::from("example2"),
                    TargetMetadata {
                        icon: Some(PathBuf::from("example2.png")),
                        romfs_dir: None,
                        description: None,
                    }
                ),
            ])
        );

        assert_eq!(
            config.tests,
            HashMap::from_iter([(
                String::from("test1"),
                TargetMetadata {
                    icon: None,
                    romfs_dir: Some(PathBuf::from("test1-romfs")),
                    description: None,
                }
            )])
        );

        assert_eq!(
            config.lib,
            Some(TargetMetadata {
                icon: Some(PathBuf::from("lib.png")),
                romfs_dir: Some(PathBuf::from("lib-romfs")),
                description: None,
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
            .find(|cfg| cfg.default.romfs_dir == Some(PathBuf::from("examples/romfs")))
            .unwrap();
    }

    #[test]
    fn merge_defaults() {
        let mut config = Cargo3DS::default();

        let first: Cargo3DS = toml! {
            romfs_dir = "first_romfs"

            examples.example1.icon = "example1.png"
            examples.example2.icon = "example2.png"

            tests.test1.romfs_dir = "test1-romfs"

            lib.romfs_dir = "lib-romfs"
        }
        .try_into()
        .unwrap();

        let next: Cargo3DS = toml! {
            examples.example1.romfs-dir = "example-dir"
            tests.test1.romfs_dir = "test1-next-romfs"
            lib.romfs_dir = "lib-override"
        }
        .try_into()
        .unwrap();

        config.merge(first);
        config.merge(next);

        let mut expected: Cargo3DS = toml! {
            romfs_dir = "first_romfs"

            examples.example1.icon = "example1.png"
            examples.example1.romfs-dir = "example-dir"

            examples.example2.icon = "example2.png"

            tests.test1.romfs_dir = "test1-next-romfs"

            lib.romfs_dir = "lib-override"
        }
        .try_into()
        .unwrap();

        // Serde parsing won't set the default but since we started from a default
        // we can just set it in the expected struct here.
        expected.default.icon = Some(PathBuf::from("icon.png"));

        assert_eq!(config, expected);
    }
}
