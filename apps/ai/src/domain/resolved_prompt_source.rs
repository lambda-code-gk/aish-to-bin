/// プロンプト素材の種類
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptSourceKind {
    Hook,
    TaskPrompt,
    Skill,
}

/// Context Builder 等に渡す解決済みプロンプト素材
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPromptSource {
    pub kind: PromptSourceKind,
    pub name: String,
    pub content: String,
    pub memory_topics: Vec<String>,
    pub preferred_mode: Option<String>,
    pub allowed_tools: Vec<String>,
    pub denied_tools: Vec<String>,
}
