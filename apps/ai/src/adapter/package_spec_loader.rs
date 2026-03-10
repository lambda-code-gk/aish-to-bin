use crate::domain::PackageSpec;
use crate::ports::outbound::PackageSpecLoader;
use common::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct StdPackageSpecLoader;

impl StdPackageSpecLoader {
    pub fn new() -> Self {
        Self
    }

    fn manifest_path(package_root: &Path) -> PathBuf {
        package_root.join("package.toml")
    }
}

#[derive(Debug, serde::Deserialize)]
struct RawPackageToml {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub default_task: Option<String>,
    pub system_hook: Option<String>,
    #[serde(default)]
    pub memory_topics: Vec<String>,
    #[serde(default)]
    pub compat: CompatSection,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CompatSection {
    pub aish: Option<String>,
}

impl PackageSpecLoader for StdPackageSpecLoader {
    fn load_package_spec(
        &self,
        package_root: &Path,
    ) -> Result<Option<PackageSpec>, Error> {
        let manifest = Self::manifest_path(package_root);
        if !manifest.exists() {
            return Ok(None);
        }
        let content = match fs::read_to_string(&manifest) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to read package manifest '{}': {}",
                    manifest.display(),
                    e
                );
                return Ok(None);
            }
        };

        let raw: RawPackageToml = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse package manifest '{}': {}",
                    manifest.display(),
                    e
                );
                return Ok(None);
            }
        };

        let root_dir = package_root.to_path_buf();
        let system_hook = raw
            .system_hook
            .as_ref()
            .map(|rel| root_dir.join(rel));

        let spec = PackageSpec {
            name: raw.name,
            version: raw.version,
            description: raw.description,
            root_dir,
            default_task: raw.default_task,
            system_hook,
            memory_topics: raw.memory_topics,
            compat_aish: raw.compat.aish,
        };
        Ok(Some(spec))
    }

    fn list_package_specs(&self, packages_root: &Path) -> Result<Vec<PackageSpec>, Error> {
        let mut specs = Vec::new();
        let entries = match fs::read_dir(packages_root) {
            Ok(e) => e,
            Err(_) => return Ok(specs),
        };
        for ent in entries.flatten() {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            match self.load_package_spec(&path)? {
                Some(spec) => specs.push(spec),
                None => {}
            }
        }
        Ok(specs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn load_single_package_spec() {
        let dir = tempdir().unwrap();
        let packages_root = dir.path().join("packages");
        fs::create_dir_all(&packages_root).unwrap();
        let pkg_dir = packages_root.join("ci-investigator");
        fs::create_dir_all(&pkg_dir).unwrap();
        let manifest = pkg_dir.join("package.toml");
        let mut f = fs::File::create(&manifest).unwrap();
        writeln!(
            f,
            r#"
name = "ci-investigator"
version = "0.1.0"
description = "Investigate CI failures for common build/test pipelines"

default_task = "investigate_ci_failure"
system_hook = "prompts/system.md"

memory_topics = ["ci", "tests", "build"]

[compat]
aish = ">=0.1.0"
"#
        )
        .unwrap();

        let loader = StdPackageSpecLoader::new();
        let spec = loader
            .load_package_spec(&pkg_dir)
            .unwrap()
            .expect("spec");
        assert_eq!(spec.name, "ci-investigator");
        assert_eq!(spec.version.as_deref(), Some("0.1.0"));
        assert_eq!(
            spec.description.as_deref(),
            Some("Investigate CI failures for common build/test pipelines")
        );
        assert_eq!(spec.default_task.as_deref(), Some("investigate_ci_failure"));
        assert!(spec
            .system_hook
            .as_ref()
            .unwrap()
            .ends_with("prompts/system.md"));
        assert_eq!(
            spec.memory_topics,
            vec!["ci".to_string(), "tests".to_string(), "build".to_string()]
        );
        assert_eq!(spec.compat_aish.as_deref(), Some(">=0.1.0"));
    }

    #[test]
    fn broken_manifest_is_skipped() {
        let dir = tempdir().unwrap();
        let packages_root = dir.path().join("packages");
        fs::create_dir_all(&packages_root).unwrap();
        let pkg_dir = packages_root.join("broken");
        fs::create_dir_all(&pkg_dir).unwrap();
        let manifest = pkg_dir.join("package.toml");
        let mut f = fs::File::create(&manifest).unwrap();
        writeln!(f, "not = [[ toml").unwrap();

        let loader = StdPackageSpecLoader::new();
        let spec = loader.load_package_spec(&pkg_dir).unwrap();
        assert!(spec.is_none());

        let list = loader.list_package_specs(&packages_root).unwrap();
        assert!(list.is_empty());
    }
}

