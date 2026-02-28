//! 機微情報フィルタの結果型とアクション

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SensitiveFilterOutcome {
    Clean,
    Masked { masked: String, verbose: String },
    /// Allow 時にヒットしたが置換しない（decisions に warn を残す）
    Hit { verbose: String },
    Deny { verbose: String },
}

/// addons 送信前の leakscan 挙動
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveAction {
    Deny,
    Mask,
    Allow,
}

impl SensitiveAction {
    /// "deny"|"mask"|"allow" を解釈。未指定時は non_interactive なら Deny、それ以外は Mask
    pub fn from_str_or_default(s: &str, non_interactive: bool) -> Self {
        match s.to_lowercase().as_str() {
            "deny" => Self::Deny,
            "mask" => Self::Mask,
            "allow" => Self::Allow,
            _ if non_interactive => Self::Deny,
            _ => Self::Mask,
        }
    }
}
