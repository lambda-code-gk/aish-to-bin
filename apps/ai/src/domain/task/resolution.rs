//! 責務: タスクパスの解決ルール（`execute` 優先 -> `.sh` フォールバック）のみ。I/O を知らない。

use std::path::{Path, PathBuf};

use crate::domain::TaskName;

/// タスク解決の結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskResolution {
    pub path: PathBuf,
}

/// `task_dir` 配下で `task_name` に対応するタスクパスを解決する純関数。
///
/// 優先順位:
/// 1. `task_dir/task_name/execute` がファイルとして存在
/// 2. `task_dir/task_name.sh` がファイルとして存在
///
/// `is_file` は呼び出し元から渡すクロージャで判定する（I/O 非依存）。
pub fn resolve_task_path<F>(
    task_dir: &Path,
    task_name: &TaskName,
    is_file: F,
) -> Option<TaskResolution>
where
    F: Fn(&Path) -> bool,
{
    let dir_execute = task_dir.join(task_name.as_dir_name()).join("execute");
    if is_file(&dir_execute) {
        return Some(TaskResolution { path: dir_execute });
    }

    let script = task_dir.join(task_name.to_script_file_name());
    if is_file(&script) {
        return Some(TaskResolution { path: script });
    }

    None
}

/// タスク名一覧を抽出する純関数。
///
/// `entries` は `(file_name, is_dir)` のタプル列。
/// adapter が `read_dir` で収集した生データを渡す。
///
/// ルール:
/// - `.` 始まりはスキップ
/// - ディレクトリ内に `execute` ファイルがあればタスク名
/// - `.sh` 拡張子のファイルはベース名がタスク名
pub fn extract_task_names<F>(entries: &[(String, bool)], has_execute: F) -> Vec<TaskName>
where
    F: Fn(&TaskName) -> bool,
{
    let mut names = Vec::new();
    for (name, is_dir) in entries {
        if name.starts_with('.') {
            continue;
        }
        if *is_dir {
            if let Ok(task_name) = TaskName::parse(name) {
                if has_execute(&task_name) {
                    names.push(task_name);
                }
            }
        } else if name.ends_with(".sh") {
            if let Some(base) = name.strip_suffix(".sh") {
                if let Ok(task_name) = TaskName::parse(base) {
                    names.push(task_name);
                }
            }
        }
    }
    names.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    names.dedup_by(|a, b| a.as_ref() == b.as_ref());
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_prefers_execute_over_sh() {
        let name = TaskName::new("build");
        let res = resolve_task_path(Path::new("/tasks"), &name, |p| {
            p == Path::new("/tasks/build/execute")
        });
        assert_eq!(res.unwrap().path, PathBuf::from("/tasks/build/execute"));
    }

    #[test]
    fn resolve_falls_back_to_sh() {
        let name = TaskName::new("deploy");
        let res = resolve_task_path(Path::new("/tasks"), &name, |p| {
            p == Path::new("/tasks/deploy.sh")
        });
        assert_eq!(res.unwrap().path, PathBuf::from("/tasks/deploy.sh"));
    }

    #[test]
    fn resolve_returns_none_when_not_found() {
        let name = TaskName::new("nope");
        let res = resolve_task_path(Path::new("/tasks"), &name, |_| false);
        assert!(res.is_none());
    }

    #[test]
    fn extract_task_names_filters_correctly() {
        let entries = vec![
            (".hidden".to_string(), true),
            ("build".to_string(), true),
            ("deploy.sh".to_string(), false),
            ("empty_dir".to_string(), true),
        ];
        let names = extract_task_names(&entries, |name| name.as_ref() == "build");
        let as_strs: Vec<String> = names.iter().map(|n| n.as_ref().to_string()).collect();
        assert_eq!(as_strs, vec!["build", "deploy"]);
    }
}
