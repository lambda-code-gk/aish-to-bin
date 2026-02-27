use crate::domain::{ConfigSource, ConfigSourceKind, Resolved};
use serde::{Deserialize, Serialize};

/// policy 関連設定（v0.6 時点では policy 用のみを対象にした最小構成）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyConfig {
    pub schema_version: u32,
    pub egress_sensitive_action: Resolved<String>,
    pub egress_hard_cap_chars: Resolved<usize>,
    pub addons_sensitive_action: Resolved<String>,
    pub run_shell_mode: Resolved<String>,
    pub run_shell_allowlist: Resolved<Vec<String>>,
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
            schema_version: 1,
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
                    "sed".to_string(),
                    "awk".to_string(),
                    "cargo".to_string(),
                    "rustc".to_string(),
                ],
                default_source("defaults.policy.run_shell_allowlist"),
            ),
            non_interactive_default: Resolved::new(
                false,
                default_source("defaults.policy.non_interactive_default"),
            ),
        }
    }
}

