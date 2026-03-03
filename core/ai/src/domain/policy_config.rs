use crate::domain::{ConfigSource, ConfigSourceKind, Resolved};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// policy 関連設定（v0.6 時点では policy 用のみを対象にした最小構成）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyConfig {
    pub schema_version: Resolved<u32>,
    pub egress_sensitive_action: Resolved<String>,
    pub egress_hard_cap_chars: Resolved<usize>,
    pub addons_sensitive_action: Resolved<String>,
    pub run_shell_mode: Resolved<String>,
    pub run_shell_allowlist: Resolved<Vec<String>>,
    /// ツール未指定時のデフォルト（allow | require_approval | deny）
    pub tool_default_mode: Resolved<String>,
    /// capability 種別ごとの mode（例: fs_read -> allow）。key は snake_case（fs_read, fs_write, exec, network, data_egress）
    pub tool_mode_by_capability: Resolved<HashMap<String, String>>,
    /// ツール名ごとの mode 上書き（policy.tools.<name>.mode）。run_shell もここで上書き可能
    pub tool_modes: Resolved<HashMap<String, String>>,
    pub non_interactive_default: Resolved<bool>,
}

impl PolicyConfig {
    /// ビルトインのデフォルト設定を返す。
    ///
    /// non_interactive のデフォルト値もここで指定するが、
    /// 実際の動作時には wiring 側で CLI の --no-interactive などから
    /// より正確な値に上書きされることを想定している。
    pub fn defaults() -> Self {
        let default_source = |ref_id: &str| ConfigSource {
            kind: ConfigSourceKind::Default,
            ref_id: ref_id.to_string(),
        };

        PolicyConfig {
            schema_version: Resolved::new(1, default_source("defaults.schema_version")),
            egress_sensitive_action: Resolved::new(
                "mask".to_string(),
                default_source("defaults.policy.egress_sensitive_action"),
            ),
            egress_hard_cap_chars: Resolved::new(
                200_000,
                default_source("defaults.policy.egress_hard_cap_chars"),
            ),
            addons_sensitive_action: Resolved::new(
                "mask".to_string(),
                default_source("defaults.policy.addons_sensitive_action"),
            ),
            run_shell_mode: Resolved::new(
                "require_approval".to_string(),
                default_source("defaults.policy.run_shell_mode"),
            ),
            run_shell_allowlist: Resolved::new(
                vec![
                    "git".to_string(),
                    "ls".to_string(),
                    "cat".to_string(),
                    "rg".to_string(),
                    "fd".to_string(),
                    "cargo".to_string(),
                ],
                default_source("defaults.policy.run_shell_allowlist"),
            ),
            tool_default_mode: Resolved::new(
                "require_approval".to_string(),
                default_source("defaults.policy.tool_default_mode"),
            ),
            tool_mode_by_capability: Resolved::new(
                HashMap::new(),
                default_source("defaults.policy.tool_mode_by_capability"),
            ),
            tool_modes: Resolved::new(HashMap::new(), default_source("defaults.policy.tool_modes")),
            non_interactive_default: Resolved::new(
                false,
                default_source("defaults.policy.non_interactive_default"),
            ),
        }
    }
}
