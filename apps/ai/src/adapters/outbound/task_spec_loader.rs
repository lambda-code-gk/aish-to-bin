use crate::domain::{TaskName, TaskSpec};
use crate::ports::outbound::TaskSpecLoader;
use common::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct StdTaskSpecLoader;

impl StdTaskSpecLoader {
    pub fn new() -> Self {
        Self
    }

    fn candidate_paths(task_root: &Path, task_name: &TaskName) -> (PathBuf, PathBuf) {
        let dir_toml = task_root.join(task_name.as_ref()).join("task.toml");
        let file_toml = task_root.join(format!("{}.toml", task_name.as_ref()));
        (dir_toml, file_toml)
    }
}

#[derive(Debug, serde::Deserialize)]
struct RawTaskToml {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub memory_topics: Vec<String>,
    pub preferred_mode: Option<String>,
}

impl TaskSpecLoader for StdTaskSpecLoader {
    fn load_task_spec(
        &self,
        task_root: &Path,
        task_name: &TaskName,
    ) -> Result<Option<TaskSpec>, Error> {
        let (dir_toml, file_toml) = Self::candidate_paths(task_root, task_name);
        let path = if dir_toml.exists() {
            Some(dir_toml)
        } else if file_toml.exists() {
            Some(file_toml)
        } else {
            None
        };
        let Some(path) = path else {
            return Ok(None);
        };

        let content = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to read task spec '{}': {}",
                    path.display(),
                    e
                );
                return Ok(None);
            }
        };

        let raw: RawTaskToml = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to parse task spec '{}': {}",
                    path.display(),
                    e
                );
                return Ok(None);
            }
        };

        let name = raw.name.unwrap_or_else(|| task_name.as_ref().to_string());
        let spec = TaskSpec {
            name: TaskName::new(name),
            description: raw.description,
            skills: raw.skills,
            memory_topics: raw.memory_topics,
            preferred_mode: raw.preferred_mode,
        };
        Ok(Some(spec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn load_directory_task_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let task_root = root.join("task.d");
        fs::create_dir_all(&task_root).unwrap();
        let task_dir = task_root.join("investigate_ci_failure");
        fs::create_dir_all(&task_dir).unwrap();
        let toml_path = task_dir.join("task.toml");
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(
            f,
            r#"
name = "investigate_ci_failure"
description = "Investigate CI failure"
skills = ["analyze_test_failure"]
memory_topics = ["ci", "tests"]
preferred_mode = "readonly"
"#
        )
        .unwrap();

        let loader = StdTaskSpecLoader::new();
        let name = TaskName::new("investigate_ci_failure");
        let spec = loader
            .load_task_spec(&task_root, &name)
            .unwrap()
            .expect("spec");
        assert_eq!(spec.name.as_ref(), "investigate_ci_failure");
        assert_eq!(spec.description.as_deref(), Some("Investigate CI failure"));
        assert_eq!(spec.skills, vec!["analyze_test_failure".to_string()]);
        assert_eq!(
            spec.memory_topics,
            vec!["ci".to_string(), "tests".to_string()]
        );
        assert_eq!(spec.preferred_mode.as_deref(), Some("readonly"));
    }

    #[test]
    fn load_file_task_spec() {
        let dir = tempdir().unwrap();
        let task_root = dir.path().join("task.d");
        fs::create_dir_all(&task_root).unwrap();
        let toml_path = task_root.join("commit_helper.toml");
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(
            f,
            r#"
description = "Commit helper"
memory_topics = ["git"]
"#
        )
        .unwrap();

        let loader = StdTaskSpecLoader::new();
        let name = TaskName::new("commit_helper");
        let spec = loader
            .load_task_spec(&task_root, &name)
            .unwrap()
            .expect("spec");
        assert_eq!(spec.name.as_ref(), "commit_helper");
        assert_eq!(spec.description.as_deref(), Some("Commit helper"));
        assert_eq!(spec.skills, Vec::<String>::new());
        assert_eq!(spec.memory_topics, vec!["git".to_string()]);
        assert_eq!(spec.preferred_mode, None);
    }

    #[test]
    fn missing_task_spec_returns_none() {
        let dir = tempdir().unwrap();
        let task_root = dir.path().join("task.d");
        fs::create_dir_all(&task_root).unwrap();

        let loader = StdTaskSpecLoader::new();
        let name = TaskName::new("unknown");
        let spec = loader.load_task_spec(&task_root, &name).unwrap();
        assert!(spec.is_none());
    }

    #[test]
    fn broken_toml_warns_and_returns_none() {
        let dir = tempdir().unwrap();
        let task_root = dir.path().join("task.d");
        fs::create_dir_all(&task_root).unwrap();
        let toml_path = task_root.join("broken.toml");
        let mut f = fs::File::create(&toml_path).unwrap();
        writeln!(f, "this = [[ is not toml").unwrap();

        let loader = StdTaskSpecLoader::new();
        let name = TaskName::new("broken");
        let spec = loader.load_task_spec(&task_root, &name).unwrap();
        assert!(spec.is_none());
    }
}

