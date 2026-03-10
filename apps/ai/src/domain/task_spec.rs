use crate::domain::TaskName;

/// task.d 配下の task 用メタデータ（task.toml）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub name: TaskName,
    pub description: Option<String>,
    pub skills: Vec<String>,
    pub memory_topics: Vec<String>,
    pub preferred_mode: Option<String>,
}
