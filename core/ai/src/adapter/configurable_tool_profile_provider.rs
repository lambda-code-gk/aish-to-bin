//! 設定に基づく ToolProfile 解決（案1: デフォルト+ツール別、案2: capability ベース）
//!
//! 解決順: policy.tools.<name>.mode > policy.tool_mode_by_capability[cap] > policy.tool_default_mode

use crate::domain::{PolicyConfig, ToolCapability, ToolMode, ToolProfile};
use crate::ports::outbound::ToolProfileProvider;
use std::sync::Arc;

/// 組み込みツール名 → capability 種別（snake_case）。未登録は capability なし → tool_default_mode
fn builtin_capability_kind(tool_name: &str) -> Option<&'static str> {
    let map: &[(&str, &str)] = &[
        ("read_file", "fs_read"),
        ("grep", "fs_read"),
        ("history_get", "fs_read"),
        ("history_search", "fs_read"),
        ("get_memory_content", "fs_read"),
        ("search_memory", "fs_read"),
        ("write_file", "fs_write"),
        ("replace_file", "fs_write"),
        ("save_memory", "fs_write"),
        ("run_shell", "exec"),
        ("queue_shell_suggestion", "exec"),
    ];
    map.iter()
        .find(|(name, _)| *name == tool_name)
        .map(|(_, cap)| *cap)
}

fn parse_mode(s: &str) -> ToolMode {
    match s.to_lowercase().as_str() {
        "allow" => ToolMode::Allow,
        "deny" => ToolMode::Deny,
        _ => ToolMode::RequireApproval,
    }
}

/// PolicyConfig と run_shell 用 allowlist から ToolProfile を動的に解決するプロバイダ
pub struct ConfigurableToolProfileProvider {
    policy_cfg: Arc<PolicyConfig>,
    /// run_shell 用（Exec capability の allowlist）。wiring で run_shell_allowlist を渡す
    run_shell_allowlist: Vec<String>,
}

impl ConfigurableToolProfileProvider {
    pub fn new(policy_cfg: Arc<PolicyConfig>, run_shell_allowlist: Vec<String>) -> Self {
        Self {
            policy_cfg,
            run_shell_allowlist,
        }
    }

    fn resolve_mode(&self, tool_name: &str) -> ToolMode {
        let cfg = &self.policy_cfg;
        // 1) ツール別オーバーライド（policy.tools.<name>.mode）。run_shell は tool_modes または run_shell_mode
        if tool_name == "run_shell" {
            let mode_str = cfg
                .tool_modes
                .value
                .get("run_shell")
                .map(String::as_str)
                .unwrap_or_else(|| cfg.run_shell_mode.value.as_str());
            return parse_mode(mode_str);
        }
        if let Some(mode_str) = cfg.tool_modes.value.get(tool_name) {
            return parse_mode(mode_str);
        }
        // 2) capability ベース
        if let Some(cap_kind) = builtin_capability_kind(tool_name) {
            if let Some(mode_str) = cfg.tool_mode_by_capability.value.get(cap_kind) {
                return parse_mode(mode_str);
            }
        }
        // 3) デフォルト
        parse_mode(&cfg.tool_default_mode.value)
    }

    fn capabilities_for(&self, tool_name: &str) -> Vec<ToolCapability> {
        if tool_name == "run_shell" {
            return vec![ToolCapability::Exec {
                allowlist: self.run_shell_allowlist.clone(),
            }];
        }
        if let Some(cap_kind) = builtin_capability_kind(tool_name) {
            return match cap_kind {
                "fs_read" => vec![ToolCapability::FsRead {
                    paths: vec!["*".to_string()],
                }],
                "fs_write" => vec![ToolCapability::FsWrite {
                    paths: vec!["*".to_string()],
                }],
                "exec" => vec![ToolCapability::Exec { allowlist: vec![] }],
                _ => vec![],
            };
        }
        vec![]
    }
}

impl ToolProfileProvider for ConfigurableToolProfileProvider {
    fn get(&self, tool_name: &str) -> ToolProfile {
        let mode = self.resolve_mode(tool_name);
        let capabilities = self.capabilities_for(tool_name);
        ToolProfile {
            tool_name: tool_name.to_string(),
            mode,
            capabilities,
            notes: Some("configurable".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ConfigSource, ConfigSourceKind, Resolved};
    use std::collections::HashMap;

    fn default_config() -> PolicyConfig {
        PolicyConfig::defaults()
    }

    fn config_with_tool_default(mode: &str) -> PolicyConfig {
        let mut c = PolicyConfig::defaults();
        c.tool_default_mode = Resolved::new(
            mode.to_string(),
            ConfigSource {
                kind: ConfigSourceKind::Default,
                ref_id: "test".to_string(),
            },
        );
        c
    }

    fn config_with_capability_mode(cap: &str, mode: &str) -> PolicyConfig {
        let mut c = PolicyConfig::defaults();
        let mut map = HashMap::new();
        map.insert(cap.to_string(), mode.to_string());
        c.tool_mode_by_capability = Resolved::new(
            map,
            ConfigSource {
                kind: ConfigSourceKind::Default,
                ref_id: "test".to_string(),
            },
        );
        c
    }

    #[test]
    fn default_run_shell_uses_run_shell_mode() {
        let cfg = Arc::new(default_config());
        let p = ConfigurableToolProfileProvider::new(cfg, vec!["ls".to_string()]);
        let profile = p.get("run_shell");
        assert_eq!(profile.mode, ToolMode::RequireApproval);
        assert_eq!(profile.capabilities.len(), 1);
    }

    #[test]
    fn tool_default_mode_allow_unknown_tool_gets_allow() {
        let cfg = Arc::new(config_with_tool_default("allow"));
        let p = ConfigurableToolProfileProvider::new(cfg, vec![]);
        let profile = p.get("unknown_plugin_tool");
        assert_eq!(profile.mode, ToolMode::Allow);
    }

    #[test]
    fn capability_fs_read_allow_read_file_gets_allow() {
        let cfg = Arc::new(config_with_capability_mode("fs_read", "allow"));
        let p = ConfigurableToolProfileProvider::new(cfg, vec![]);
        let profile = p.get("read_file");
        assert_eq!(profile.mode, ToolMode::Allow);
    }

    #[test]
    fn tool_override_beats_capability() {
        let mut c = PolicyConfig::defaults();
        let mut tool_modes = HashMap::new();
        tool_modes.insert("read_file".to_string(), "deny".to_string());
        c.tool_modes = Resolved::new(
            tool_modes,
            ConfigSource {
                kind: ConfigSourceKind::Default,
                ref_id: "test".to_string(),
            },
        );
        let mut cap_map = HashMap::new();
        cap_map.insert("fs_read".to_string(), "allow".to_string());
        c.tool_mode_by_capability = Resolved::new(
            cap_map,
            ConfigSource {
                kind: ConfigSourceKind::Default,
                ref_id: "test".to_string(),
            },
        );
        let p = ConfigurableToolProfileProvider::new(Arc::new(c), vec![]);
        let profile = p.get("read_file");
        assert_eq!(profile.mode, ToolMode::Deny);
    }
}
