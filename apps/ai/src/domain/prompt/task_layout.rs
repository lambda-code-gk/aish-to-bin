//! 責務: タスクのレイアウト形状（Directory / File）と prompt パスの決定のみ。I/O を知らない。

use crate::domain::TaskName;
use std::path::{Path, PathBuf};

/// タスクの物理レイアウト形状
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// `task_name/execute` 形式のディレクトリ
    Directory,
    /// `task_name.sh` 形式の単一ファイル
    File,
}

/// TaskKind に基づき task prompt ファイルのパスを返す純関数。
///
/// - Directory -> `task_root/task_name/prompt.md`
/// - File      -> `task_root/task_name.prompt.md`
pub fn task_prompt_path(task_root: &Path, task_name: &TaskName, kind: TaskKind) -> PathBuf {
    match kind {
        TaskKind::Directory => task_root.join(task_name.as_ref()).join("prompt.md"),
        TaskKind::File => task_root.join(format!("{}.prompt.md", task_name.as_ref())),
    }
}

/// `task_root` 配下で `task_name` に対応する TaskKind を判定する純関数。
///
/// - `task_name/execute` がファイルとして存在 -> Directory
/// - `task_name.sh` がファイルとして存在 -> File
/// - どちらもなければ None
///
/// `exists` / `is_file` は呼び出し元から渡すクロージャで判定する（I/O 非依存）。
pub fn detect_task_kind<F>(task_root: &Path, task_name: &str, is_file: F) -> Option<TaskKind>
where
    F: Fn(&Path) -> bool,
{
    let dir_execute = task_root.join(task_name).join("execute");
    if is_file(&dir_execute) {
        return Some(TaskKind::Directory);
    }
    let script = task_root.join(format!("{}.sh", task_name));
    if is_file(&script) {
        return Some(TaskKind::File);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_prompt_path_directory() {
        let name = TaskName::new("build");
        let p = task_prompt_path(Path::new("/tasks"), &name, TaskKind::Directory);
        assert_eq!(p, PathBuf::from("/tasks/build/prompt.md"));
    }

    #[test]
    fn task_prompt_path_file() {
        let name = TaskName::new("build");
        let p = task_prompt_path(Path::new("/tasks"), &name, TaskKind::File);
        assert_eq!(p, PathBuf::from("/tasks/build.prompt.md"));
    }

    #[test]
    fn detect_task_kind_directory_preferred() {
        let kind = detect_task_kind(Path::new("/tasks"), "build", |p| {
            p == Path::new("/tasks/build/execute")
        });
        assert_eq!(kind, Some(TaskKind::Directory));
    }

    #[test]
    fn detect_task_kind_file_fallback() {
        let kind = detect_task_kind(Path::new("/tasks"), "build", |p| {
            p == Path::new("/tasks/build.sh")
        });
        assert_eq!(kind, Some(TaskKind::File));
    }

    #[test]
    fn detect_task_kind_none() {
        let kind = detect_task_kind(Path::new("/tasks"), "build", |_| false);
        assert_eq!(kind, None);
    }
}
