use crate::domain::{ResolvedPromptSource, TaskName, TaskOriginInfo, TaskSpec};
use common::error::Error;

/// hooks / task prompt / skill prompt からプロンプト素材を解決するポート
pub trait PromptSourceResolver: Send + Sync {
    /// タスク実行時に使用するプロンプト素材を解決する。
    ///
    /// - hooks/system_prompt
    /// - task prompt（prompt.md）
    /// - task.toml で列挙された skills
    ///
    /// の順で ResolvedPromptSource を返す。タスクが存在する場合は TaskOriginInfo も返す。
    fn resolve_for_task(
        &self,
        task_name: &TaskName,
        task_spec: Option<&TaskSpec>,
    ) -> Result<(Vec<ResolvedPromptSource>, Option<TaskOriginInfo>), Error>;
}
