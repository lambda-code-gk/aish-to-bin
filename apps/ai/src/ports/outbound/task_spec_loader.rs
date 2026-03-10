use crate::domain::{TaskName, TaskSpec};
use common::error::Error;
use std::path::Path;

/// task.d 配下の task.toml から TaskSpec を読み込むポート
pub trait TaskSpecLoader: Send + Sync {
    /// 指定された task root（task.d）とタスク名から TaskSpec を読み込む。
    ///
    /// - ディレクトリタスク: <task_root>/<name>/task.toml
    /// - 単一ファイルタスク: <task_root>/<name>.toml
    ///
    /// ファイルが存在しない場合や、壊れている場合は Ok(None) を返す。
    fn load_task_spec(
        &self,
        task_root: &Path,
        task_name: &TaskName,
    ) -> Result<Option<TaskSpec>, Error>;
}
