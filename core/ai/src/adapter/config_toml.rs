use serde::Deserialize;

/// TOML から読み取る policy セクション
#[derive(Debug, Clone, Deserialize)]
pub struct ParsedPolicyToml {
    pub schema_version: Option<u32>,
    pub policy: Option<PolicySection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicySection {
    pub egress_sensitive_action: Option<String>,
    pub egress_hard_cap_chars: Option<usize>,
    pub addons_sensitive_action: Option<String>,
    pub tools: Option<PolicyToolsSection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyToolsSection {
    pub run_shell: Option<PolicyRunShellSection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyRunShellSection {
    pub mode: Option<String>,
    pub allowlist: Option<Vec<String>>,
}

/// policy 用 TOML をパースするヘルパ
pub fn parse_policy_toml(input: &str) -> Result<ParsedPolicyToml, common::error::Error> {
    toml::from_str::<ParsedPolicyToml>(input)
        .map_err(|e| common::error::Error::Json(e.to_string()))
}

