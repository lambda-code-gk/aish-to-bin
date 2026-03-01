use serde::Deserialize;
use std::collections::HashMap;

/// TOML から読み取る policy セクション
#[derive(Debug, Clone, Deserialize)]
pub struct ParsedPolicyToml {
    pub schema_version: Option<u32>,
    pub policy: Option<ParsedPolicySection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParsedPolicySection {
    pub egress_sensitive_action: Option<String>,
    pub egress_hard_cap_chars: Option<usize>,
    pub addons_sensitive_action: Option<String>,
    /// policy.tool_default_mode（allow | require_approval | deny）
    pub tool_default_mode: Option<String>,
    /// policy.tool_mode_by_capability（例: fs_read = "allow"）
    pub tool_mode_by_capability: Option<HashMap<String, String>>,
    pub tools: Option<HashMap<String, ParsedToolEntry>>,
}

/// policy.tools.<name> の1エントリ（run_shell は mode + allowlist、他は mode のみ）
#[derive(Debug, Clone, Deserialize)]
pub struct ParsedToolEntry {
    pub mode: Option<String>,
    pub allowlist: Option<Vec<String>>,
}

/// 後方互換: 従来の [policy.tools] のみで run_shell だけ書く場合の型（パースは HashMap に統一したため未使用だが参照用）
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ParsedToolsSection {
    pub run_shell: Option<ParsedRunShellSection>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ParsedRunShellSection {
    pub mode: Option<String>,
    pub allowlist: Option<Vec<String>>,
}

/// policy 用 TOML をパースするヘルパ
pub fn parse_policy_toml(input: &str) -> Result<ParsedPolicyToml, common::error::Error> {
    toml::from_str::<ParsedPolicyToml>(input).map_err(|e| common::error::Error::Json(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tool_default_mode_and_tool_mode_by_capability() {
        let toml = r#"
schema_version = 1
[policy]
tool_default_mode = "allow"
[policy.tool_mode_by_capability]
fs_read = "allow"
fs_write = "require_approval"
exec = "require_approval"
"#;
        let p = parse_policy_toml(toml).unwrap();
        let policy = p.policy.as_ref().unwrap();
        assert_eq!(policy.tool_default_mode.as_deref(), Some("allow"));
        let cap = policy.tool_mode_by_capability.as_ref().unwrap();
        assert_eq!(cap.get("fs_read").map(String::as_str), Some("allow"));
        assert_eq!(cap.get("fs_write").map(String::as_str), Some("require_approval"));
    }

    #[test]
    fn parse_tools_as_map_run_shell_and_read_file() {
        let toml = r#"
schema_version = 1
[policy.tools.run_shell]
mode = "require_approval"
allowlist = ["ls", "cat"]
[policy.tools.read_file]
mode = "allow"
"#;
        let p = parse_policy_toml(toml).unwrap();
        let tools = p.policy.as_ref().unwrap().tools.as_ref().unwrap();
        let run_shell = tools.get("run_shell").unwrap();
        assert_eq!(run_shell.mode.as_deref(), Some("require_approval"));
        assert_eq!(
            run_shell.allowlist.as_ref(),
            Some(&vec!["ls".to_string(), "cat".to_string()])
        );
        let read_file = tools.get("read_file").unwrap();
        assert_eq!(read_file.mode.as_deref(), Some("allow"));
        assert!(read_file.allowlist.is_none());
    }
}
