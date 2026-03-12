//! 承認・policy 判定・allow/deny のドメインモデルと判断ロジック（純関数）

pub mod policy;
pub mod policy_chain;
pub mod policy_config;
pub mod policy_explain;
pub mod policy_rule;
pub mod shell_command_policy;
pub mod tool_summary;
pub mod rules;

pub use policy::*;
pub use policy_chain::PolicyChain;
pub use policy_config::*;
pub use policy_explain::*;
pub use policy_rule::*;
pub use shell_command_policy::is_shell_command_allowed;
pub use tool_summary::{tool_summary_preview, truncate_chars};
pub use rules::{ShellAllowlistRule, ToolModeRule};
