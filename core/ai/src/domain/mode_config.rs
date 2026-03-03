//! モード設定（システムプロンプト・プロファイル・ツールのプリセット）
//!
//! モード名で一括切り替えするための設定。mode.d/<name>.json から読み込む。

use serde::Deserialize;

use crate::domain::AgentMode;

/// 1 モード分の設定（JSON からデシリアライズ）
#[derive(Debug, Clone, Default)]
pub struct ModeConfig {
    /// このモードで使うシステムプロンプト（-S 未指定時に使用）。
    pub system: Option<String>,
    /// デフォルトプロファイル名（例: gemini, echo）
    pub profile: Option<String>,
    /// デフォルトモデル名。省略時はプロファイルのデフォルト
    pub model: Option<String>,
    /// 有効にするツール名のリスト。未指定時は全ツール。指定時はこのリストのみ。
    pub tools: Option<Vec<String>>,
    /// Agent の挙動設定（任意）。未指定時は既定の挙動（Act, max_queries=2）。
    pub agent: Option<ModeAgentConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct ModeAgentConfig {
    pub mode: Option<AgentMode>,
    pub max_queries: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ModeConfigRaw {
    system: Option<String>,
    profile: Option<String>,
    model: Option<String>,
    tools: Option<Vec<String>>,
    agent: Option<ModeAgentConfigRaw>,
}

#[derive(Debug, Deserialize)]
struct ModeAgentConfigRaw {
    mode: Option<AgentMode>,
    max_queries: Option<usize>,
}

impl ModeConfig {
    pub fn parse_json(json: &str) -> Result<Self, serde_json::Error> {
        let raw: ModeConfigRaw = serde_json::from_str(json)?;
        Ok(Self {
            system: raw.system,
            profile: raw.profile,
            model: raw.model,
            tools: raw.tools,
            agent: raw.agent.map(|a| ModeAgentConfig {
                mode: a.mode,
                max_queries: a.max_queries,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let c = ModeConfig::parse_json("{}").unwrap();
        assert!(c.system.is_none());
        assert!(c.profile.is_none());
        assert!(c.model.is_none());
        assert!(c.tools.is_none());
        assert!(c.agent.is_none());
    }

    #[test]
    fn test_parse_plan_mode() {
        let json = r#"{
            "system": "You are a planning assistant.",
            "profile": "echo",
            "tools": ["read_file", "grep"],
            "agent": {
                "mode": "plan",
                "max_queries": 1
            }
        }"#;
        let c = ModeConfig::parse_json(json).unwrap();
        assert_eq!(c.system.as_deref(), Some("You are a planning assistant."));
        assert_eq!(c.profile.as_deref(), Some("echo"));
        assert!(c.model.is_none());
        assert_eq!(
            c.tools.as_ref().map(|v| v.as_slice()),
            Some(&["read_file".to_string(), "grep".to_string()][..])
        );
        let agent = c.agent.as_ref().expect("agent config");
        assert_eq!(agent.mode, Some(AgentMode::Plan));
        assert_eq!(agent.max_queries, Some(1));
    }

    #[test]
    fn test_parse_readonly_mode() {
        let json = r#"{
            "system": "You have read-only access.",
            "tools": ["read_file", "grep", "history_get", "history_search", "get_memory_content", "search_memory"]
        }"#;
        let c = ModeConfig::parse_json(json).unwrap();
        assert_eq!(c.system.as_deref(), Some("You have read-only access."));
        assert!(c.profile.is_none());
        assert!(c.model.is_none());
        let tools = c.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 6);
        assert!(tools.contains(&"read_file".to_string()));
        assert!(tools.contains(&"search_memory".to_string()));
        assert!(!tools.contains(&"run_shell".to_string()));
        assert!(c.agent.is_none());
    }
}
