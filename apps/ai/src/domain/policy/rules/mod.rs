//! 責務: tool 用ポリシールール（mode / shell allowlist）の共通ヘルパと個別 Rule 実装をまとめる。I/O を持たない。

use crate::domain::PolicyDecision;

pub mod shell_allowlist_rule;
pub mod tool_mode_rule;

pub use shell_allowlist_rule::ShellAllowlistRule;
pub use tool_mode_rule::ToolModeRule;

/// PolicyDecision の共通初期値を組み立てるヘルパ。
fn base_decision(scope: &str, subject: &str, status: &str, reason: &str) -> PolicyDecision {
    PolicyDecision {
        v: 1,
        scope: scope.to_string(),
        subject: subject.to_string(),
        status: status.to_string(),
        reason: reason.to_string(),
        details: serde_json::json!({}),
    }
}

/// tool 要約などで使う文字数上限。
pub const TOOL_SUMMARY_MAX_CHARS: usize = 200;
