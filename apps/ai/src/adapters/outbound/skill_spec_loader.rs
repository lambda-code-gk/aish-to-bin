use crate::domain::SkillSpec;
use crate::ports::outbound::SkillSpecLoader;
use common::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct StdSkillSpecLoader;

impl StdSkillSpecLoader {
    pub fn new() -> Self {
        Self
    }

    fn skill_dir(skills_root: &Path, skill_name: &str) -> PathBuf {
        skills_root.join(skill_name)
    }
}

#[derive(Debug, serde::Deserialize)]
struct RawSkillToml {
    pub name: String,
    pub description: Option<String>,
    pub instructions: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub denied_tools: Vec<String>,
    #[serde(default)]
    pub memory_topics: Vec<String>,
    pub preferred_mode: Option<String>,
}

impl SkillSpecLoader for StdSkillSpecLoader {
    fn load_skill_spec(
        &self,
        skills_root: &Path,
        skill_name: &str,
    ) -> Result<Option<SkillSpec>, Error> {
        let dir = Self::skill_dir(skills_root, skill_name);
        let toml_path = dir.join("skill.toml");
        if !toml_path.exists() {
            return Ok(None);
        }
        let content = match fs::read_to_string(&toml_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to read skill spec '{}': {}",
                    toml_path.display(),
                    e
                );
                return Ok(None);
            }
        };

        let raw: RawSkillToml = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse skill spec '{}': {}",
                    toml_path.display(),
                    e
                );
                return Ok(None);
            }
        };

        let instructions_path = dir.join(&raw.instructions);
        let spec = SkillSpec {
            name: raw.name,
            description: raw.description,
            instructions_path,
            allowed_tools: raw.allowed_tools,
            denied_tools: raw.denied_tools,
            memory_topics: raw.memory_topics,
            preferred_mode: raw.preferred_mode,
        };
        Ok(Some(spec))
    }

    fn list_skill_specs(&self, skills_root: &Path) -> Result<Vec<SkillSpec>, Error> {
        let mut specs = Vec::new();
        let entries = match fs::read_dir(skills_root) {
            Ok(e) => e,
            Err(_) => return Ok(specs),
        };
        for ent in entries.flatten() {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            match self.load_skill_spec(skills_root, name)? {
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
    fn load_single_skill_spec() {
        let dir = tempdir().unwrap();
        let skills_root = dir.path().join("skills");
        fs::create_dir_all(&skills_root).unwrap();
        let skill_dir = skills_root.join("analyze_test_failure");
        fs::create_dir_all(&skill_dir).unwrap();
        let toml_path = skill_dir.join("skill.toml");
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(
            f,
            r#"
name = "analyze_test_failure"
description = "Analyze failing test logs"
instructions = "prompt.md"
allowed_tools = ["read_file", "grep"]
denied_tools = ["write_file"]
memory_topics = ["ci", "tests"]
preferred_mode = "readonly"
"#
        )
        .unwrap();
        fs::write(skill_dir.join("prompt.md"), "Prompt body").unwrap();

        let loader = StdSkillSpecLoader::new();
        let spec = loader
            .load_skill_spec(&skills_root, "analyze_test_failure")
            .unwrap()
            .expect("spec");
        assert_eq!(spec.name, "analyze_test_failure");
        assert_eq!(
            spec.description.as_deref(),
            Some("Analyze failing test logs")
        );
        assert!(spec.instructions_path.ends_with("prompt.md"));
        assert_eq!(
            spec.allowed_tools,
            vec!["read_file".to_string(), "grep".to_string()]
        );
        assert_eq!(spec.denied_tools, vec!["write_file".to_string()]);
        assert_eq!(
            spec.memory_topics,
            vec!["ci".to_string(), "tests".to_string()]
        );
        assert_eq!(spec.preferred_mode.as_deref(), Some("readonly"));
    }

    #[test]
    fn list_skill_specs_reads_all() {
        let dir = tempdir().unwrap();
        let skills_root = dir.path().join("skills");
        fs::create_dir_all(&skills_root).unwrap();

        for name in &["one", "two"] {
            let skill_dir = skills_root.join(name);
            fs::create_dir_all(&skill_dir).unwrap();
            let toml_path = skill_dir.join("skill.toml");
            let mut f = fs::File::create(&toml_path).unwrap();
            writeln!(
                f,
                r#"
name = "{}"
instructions = "prompt.md"
"#,
                name
            )
            .unwrap();
            fs::write(skill_dir.join("prompt.md"), "Prompt").unwrap();
        }

        let loader = StdSkillSpecLoader::new();
        let specs = loader.list_skill_specs(&skills_root).unwrap();
        let names: Vec<String> = specs.into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"one".to_string()));
        assert!(names.contains(&"two".to_string()));
    }

    #[test]
    fn broken_skill_toml_is_skipped() {
        let dir = tempdir().unwrap();
        let skills_root = dir.path().join("skills");
        fs::create_dir_all(&skills_root).unwrap();
        let skill_dir = skills_root.join("broken");
        fs::create_dir_all(&skill_dir).unwrap();
        let toml_path = skill_dir.join("skill.toml");
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(f, "not = [[ toml").unwrap();

        let loader = StdSkillSpecLoader::new();
        let specs = loader.list_skill_specs(&skills_root).unwrap();
        assert!(specs.is_empty());
    }
}

