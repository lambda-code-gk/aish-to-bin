//! 設定に基づく ToolProfile 解決。
//!
//! 解決順:
//! 1. policy.tools.<name>.mode
//! 2. policy.tool_mode_by_capability[...]（built-in capability / external capability hint の両方）
//! 3. external tool の default_tool_mode_hint または RequireApproval
//! 4. built-in の既定ロジック / その他の tool_default_mode

use crate::domain::{ExternalToolPolicyIndex, PolicyConfig, ToolCapability, ToolMode, ToolProfile};
use crate::ports::outbound::ToolProfileProvider;
use std::sync::Arc;

/// 組み込みツール名 → capability 種別（snake_case）。
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

fn mode_severity(mode: &ToolMode) -> u8 {
    match mode {
        ToolMode::Deny => 3,
        ToolMode::RequireApproval => 2,
        ToolMode::Allow => 1,
    }
}

/// PolicyConfig と run_shell 用 allowlist / external tool index から ToolProfile を動的に解決するプロバイダ
pub struct ConfigurableToolProfileProvider {
    policy_cfg: Arc<PolicyConfig>,
    /// run_shell 用（Exec capability の allowlist）。wiring で run_shell_allowlist を渡す
    run_shell_allowlist: Vec<String>,
    /// external tool の policy hint index（sanitized name → metadata）
    external_index: Option<Arc<ExternalToolPolicyIndex>>,
}

impl ConfigurableToolProfileProvider {
    pub fn new(
        policy_cfg: Arc<PolicyConfig>,
        run_shell_allowlist: Vec<String>,
        external_index: Option<Arc<ExternalToolPolicyIndex>>,
    ) -> Self {
        Self {
            policy_cfg,
            run_shell_allowlist,
            external_index,
        }
    }

    fn external_capability_kinds(&self, tool_name: &str) -> Vec<String> {
        self.external_index
            .as_ref()
            .and_then(|idx| idx.get(tool_name))
            .map(|hint| hint.capabilities_hint.clone())
            .unwrap_or_default()
    }

    fn resolve_mode_from_capability_kinds(&self, kinds: &[String]) -> Option<ToolMode> {
        let cfg = &self.policy_cfg;
        let mut chosen: Option<ToolMode> = None;
        for k in kinds {
            if let Some(mode_str) = cfg.tool_mode_by_capability.value.get(k) {
                let mode = parse_mode(mode_str);
                match &chosen {
                    None => chosen = Some(mode),
                    Some(prev) => {
                        if mode_severity(&mode) > mode_severity(prev) {
                            chosen = Some(mode);
                        }
                    }
                }
            }
        }
        chosen
    }

    fn is_external_tool(&self, tool_name: &str) -> bool {
        self.external_index
            .as_ref()
            .map(|idx| idx.is_external(tool_name))
            .unwrap_or(false)
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

        // 2) capability ベース（built-in capability + external capability hint）
        let mut cap_kinds: Vec<String> = Vec::new();
        if let Some(cap) = builtin_capability_kind(tool_name) {
            cap_kinds.push(cap.to_string());
        }
        cap_kinds.extend(self.external_capability_kinds(tool_name));
        if let Some(mode) = self.resolve_mode_from_capability_kinds(&cap_kinds) {
            return mode;
        }

        // 3) external default（global default は external には継承しない）
        if self.is_external_tool(tool_name) {
            if let Some(idx) = &self.external_index {
                if let Some(hint) = idx.get(tool_name) {
                    if let Some(ref s) = hint.default_tool_mode_hint {
                        return parse_mode(s);
                    }
                }
            }
            return ToolMode::RequireApproval;
        }

        // 4) built-in / unknown のデフォルト
        if builtin_capability_kind(tool_name).is_some() {
            // built-in は従来どおり tool_default_mode を適用
            parse_mode(&cfg.tool_default_mode.value)
        } else {
            // unknown tool は fail-closed
            ToolMode::RequireApproval
        }
    }

    fn capabilities_for(&self, tool_name: &str) -> Vec<ToolCapability> {
        if tool_name == "run_shell" {
            return vec![ToolCapability::Exec {
                allowlist: self.run_shell_allowlist.clone(),
            }];
        }
        // built-in
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
        // external
        if let Some(idx) = &self.external_index {
            if let Some(hint) = idx.get(tool_name) {
                let mut caps = Vec::new();
                for k in &hint.capabilities_hint {
                    match k.as_str() {
                        "fs_read" => caps.push(ToolCapability::FsRead {
                            paths: vec!["*".to_string()],
                        }),
                        "fs_write" => caps.push(ToolCapability::FsWrite {
                            paths: vec!["*".to_string()],
                        }),
                        "exec" => caps.push(ToolCapability::Exec { allowlist: vec![] }),
                        "network" => caps.push(ToolCapability::Network { allow: true }),
                        _ => {
                            // unknown hint は無視（conservative）
                        }
                    }
                }
                if !caps.is_empty() {
                    return caps;
                }
            }
        }
        Vec::new()
    }
}

impl ToolProfileProvider for ConfigurableToolProfileProvider {
    fn get(&self, tool_name: &str) -> ToolProfile {
        let mode = self.resolve_mode(tool_name);
        let capabilities = self.capabilities_for(tool_name);
        let notes = if let Some(idx) = &self.external_index {
            if let Some(hint) = idx.get(tool_name) {
                let mut msg = format!(
                    "external server={} tool={}",
                    hint.server_id, hint.canonical_tool_id
                );
                if let Some(src) = &hint.source {
                    msg.push_str(&format!(" source={}", src));
                }
                Some(msg)
            } else {
                Some("configurable".to_string())
            }
        } else {
            Some("configurable".to_string())
        };
        ToolProfile {
            tool_name: tool_name.to_string(),
            mode,
            capabilities,
            notes,
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
        let p = ConfigurableToolProfileProvider::new(cfg, vec!["ls".to_string()], None);
        let profile = p.get("run_shell");
        assert_eq!(profile.mode, ToolMode::RequireApproval);
        assert_eq!(profile.capabilities.len(), 1);
    }

    #[test]
    fn tool_default_mode_allow_does_not_open_unknown_tool() {
        let cfg = Arc::new(config_with_tool_default("allow"));
        let p = ConfigurableToolProfileProvider::new(cfg, vec![], None);
        let profile = p.get("unknown_plugin_tool");
        // unknown tool は fail-closed で RequireApproval
        assert_eq!(profile.mode, ToolMode::RequireApproval);
    }

    #[test]
    fn capability_fs_read_allow_read_file_gets_allow() {
        let cfg = Arc::new(config_with_capability_mode("fs_read", "allow"));
        let p = ConfigurableToolProfileProvider::new(cfg, vec![], None);
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
        let p = ConfigurableToolProfileProvider::new(Arc::new(c), vec![], None);
        let profile = p.get("read_file");
        assert_eq!(profile.mode, ToolMode::Deny);
    }

    #[test]
    fn external_tool_defaults_to_require_approval_even_if_global_allow() {
        use crate::domain::{ExternalToolPolicyHint, ExternalToolPolicyIndex};

        let cfg = Arc::new(config_with_tool_default("allow"));
        let mut map = std::collections::HashMap::new();
        map.insert(
            "acme_search".to_string(),
            ExternalToolPolicyHint {
                tool_name: "acme_search".to_string(),
                canonical_tool_id: "acme.search".to_string(),
                server_id: "acme-docs".to_string(),
                source: Some("/tmp/plugin.toml".to_string()),
                default_tool_mode_hint: None,
                capabilities_hint: vec![],
                notes: None,
            },
        );
        let index = Arc::new(ExternalToolPolicyIndex::new(map));
        let p = ConfigurableToolProfileProvider::new(cfg, vec![], Some(index));
        let profile = p.get("acme_search");
        assert_eq!(profile.mode, ToolMode::RequireApproval);
    }

    #[test]
    fn external_tool_default_mode_hint_overrides_require_approval() {
        use crate::domain::{ExternalToolPolicyHint, ExternalToolPolicyIndex};

        let cfg = Arc::new(default_config());
        let mut map = std::collections::HashMap::new();
        map.insert(
            "acme_search".to_string(),
            ExternalToolPolicyHint {
                tool_name: "acme_search".to_string(),
                canonical_tool_id: "acme.search".to_string(),
                server_id: "acme-docs".to_string(),
                source: None,
                default_tool_mode_hint: Some("allow".to_string()),
                capabilities_hint: vec!["network".to_string()],
                notes: None,
            },
        );
        let index = Arc::new(ExternalToolPolicyIndex::new(map));
        let p = ConfigurableToolProfileProvider::new(cfg, vec![], Some(index));
        let profile = p.get("acme_search");
        assert_eq!(profile.mode, ToolMode::Allow);
    }

    #[test]
    fn external_capability_hint_respects_capability_policy() {
        use crate::domain::{ExternalToolPolicyHint, ExternalToolPolicyIndex};

        let mut c = PolicyConfig::defaults();
        let mut by_cap = HashMap::new();
        by_cap.insert("network".to_string(), "deny".to_string());
        c.tool_mode_by_capability = Resolved::new(
            by_cap,
            ConfigSource {
                kind: ConfigSourceKind::Default,
                ref_id: "test".to_string(),
            },
        );
        let cfg = Arc::new(c);

        let mut map = std::collections::HashMap::new();
        map.insert(
            "acme_search".to_string(),
            ExternalToolPolicyHint {
                tool_name: "acme_search".to_string(),
                canonical_tool_id: "acme.search".to_string(),
                server_id: "acme-docs".to_string(),
                source: None,
                default_tool_mode_hint: Some("require_approval".to_string()),
                capabilities_hint: vec!["network".to_string()],
                notes: None,
            },
        );
        let index = Arc::new(ExternalToolPolicyIndex::new(map));
        let p = ConfigurableToolProfileProvider::new(cfg, vec![], Some(index));
        let profile = p.get("acme_search");
        assert_eq!(profile.mode, ToolMode::Deny);
    }

    #[test]
    fn unknown_external_capability_hint_is_ignored() {
        use crate::domain::{ExternalToolPolicyHint, ExternalToolPolicyIndex};

        let cfg = Arc::new(default_config());
        let mut map = std::collections::HashMap::new();
        map.insert(
            "acme_search".to_string(),
            ExternalToolPolicyHint {
                tool_name: "acme_search".to_string(),
                canonical_tool_id: "acme.search".to_string(),
                server_id: "acme-docs".to_string(),
                source: None,
                default_tool_mode_hint: None,
                capabilities_hint: vec!["unknown_cap".to_string()],
                notes: None,
            },
        );
        let index = Arc::new(ExternalToolPolicyIndex::new(map));
        let p = ConfigurableToolProfileProvider::new(cfg, vec![], Some(index));
        let profile = p.get("acme_search");
        // hint が未知なので capability ベースでは決まらず、external default RequireApproval になる
        assert_eq!(profile.mode, ToolMode::RequireApproval);
        assert!(profile.capabilities.is_empty());
    }
}
