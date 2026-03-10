use std::path::PathBuf;

/// skill.toml + instructions ファイルのメタデータ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSpec {
    pub name: String,
    pub description: Option<String>,
    pub instructions_path: PathBuf,
    pub allowed_tools: Vec<String>,
    pub denied_tools: Vec<String>,
    pub memory_topics: Vec<String>,
    pub preferred_mode: Option<String>,
}
