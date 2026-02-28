use serde::Deserialize;

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
    pub tools: Option<ParsedToolsSection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParsedToolsSection {
    pub run_shell: Option<ParsedRunShellSection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParsedRunShellSection {
    pub mode: Option<String>,
    pub allowlist: Option<Vec<String>>,
}

/// policy 用 TOML をパースするヘルパ
pub fn parse_policy_toml(input: &str) -> Result<ParsedPolicyToml, common::error::Error> {
    toml::from_str::<ParsedPolicyToml>(input).map_err(|e| common::error::Error::Json(e.to_string()))
}
