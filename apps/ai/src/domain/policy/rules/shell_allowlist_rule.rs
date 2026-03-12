//! 責務: shell 実行ツール用の allowlist 規約に基づき、Allow / RequireApproval / Deny を決める純粋な Rule 実装。I/O を持たない。

use crate::domain::{
    is_shell_command_allowed, truncate_chars, PolicyVerdict, RuleVerdict, ToolCapability,
    ToolPolicyRule, ToolProfile,
};
use common::error::Error;
use common::tool::ToolContext;

use super::{base_decision, TOOL_SUMMARY_MAX_CHARS};

/// run_shell 専用 allowlist ルール
pub struct ShellAllowlistRule {
    pub shell_tool_name: &'static str,
}

impl ToolPolicyRule for ShellAllowlistRule {
    fn name(&self) -> &'static str {
        "shell_allowlist"
    }

    fn evaluate(
        &self,
        tool_name: &str,
        tool_args: &serde_json::Value,
        profile: &ToolProfile,
        tool_ctx: &ToolContext,
        non_interactive: bool,
    ) -> Result<RuleVerdict<ToolContext>, Error> {
        if tool_name != self.shell_tool_name {
            return Ok(RuleVerdict::NoMatch);
        }

        let command = tool_args
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        let mut profile_allowlist: Vec<String> = Vec::new();
        for cap in &profile.capabilities {
            if let ToolCapability::Exec { allowlist } = cap {
                profile_allowlist.extend(allowlist.iter().cloned());
            }
        }

        let allowed =
            is_shell_command_allowed(command, &profile_allowlist, &tool_ctx.command_allow_rules);

        if allowed {
            let mut decision = base_decision("tool", tool_name, "allowed", "shell_allowlist");
            decision.details = serde_json::json!({
                "command": truncate_chars(command, TOOL_SUMMARY_MAX_CHARS),
            });
            return Ok(RuleVerdict::Verdict(PolicyVerdict::Allow {
                value: tool_ctx.clone(),
                decision,
            }));
        }

        if non_interactive {
            let mut decision = base_decision(
                "tool",
                tool_name,
                "blocked",
                "shell_not_allowlisted_non_interactive",
            );
            decision.details = serde_json::json!({
                "command": truncate_chars(command, TOOL_SUMMARY_MAX_CHARS),
            });
            return Ok(RuleVerdict::Verdict(PolicyVerdict::Deny { decision }));
        }

        let mut decision = base_decision("tool", tool_name, "warn", "shell_approval_required");
        decision.details = serde_json::json!({
            "command": truncate_chars(command, TOOL_SUMMARY_MAX_CHARS),
        });
        Ok(RuleVerdict::Verdict(PolicyVerdict::RequireApproval {
            value: tool_ctx.clone().with_allow_unsafe(true),
            decision,
            prompt: command.to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PolicyVerdict, RuleVerdict, ToolMode, ToolPolicyRule};
    use common::tool::CommandAllowRule;

    fn profile_with_exec_allowlist(
        tool_name: &str,
        allowlist: &[&str],
        mode: ToolMode,
    ) -> ToolProfile {
        ToolProfile {
            tool_name: tool_name.to_string(),
            mode,
            capabilities: vec![ToolCapability::Exec {
                allowlist: allowlist.iter().map(|s| s.to_string()).collect(),
            }],
            notes: None,
        }
    }

    #[test]
    fn shell_allowlist_rule_details_command_truncates_by_chars() {
        let rule = ShellAllowlistRule {
            shell_tool_name: "run_shell",
        };

        // allowlist に一致させて allowed 分岐へ
        let profile = profile_with_exec_allowlist("run_shell", &["echo"], ToolMode::Allow);
        let tool_ctx = ToolContext::new(None);

        // 先頭トークンは echo、引数に長い日本語を入れて details.command が切られることを確認
        let cmd = format!("echo {}", "あ".repeat(250));
        let tool_args = serde_json::json!({"command": cmd});

        let verdict = rule
            .evaluate("run_shell", &tool_args, &profile, &tool_ctx, false)
            .expect("evaluate should succeed");

        match verdict {
            RuleVerdict::Verdict(PolicyVerdict::Allow { decision, .. }) => {
                let command = decision
                    .details
                    .get("command")
                    .and_then(|v| v.as_str())
                    .expect("details.command should be a string");
                assert!(command.ends_with("...(truncated)"));
                assert!(command.chars().count() <= 200 + "...(truncated)".chars().count());
            }
            _ => panic!("expected Allow"),
        }
    }

    #[test]
    fn shell_allowlist_rule_uses_command_rules_for_allow() {
        let rule = ShellAllowlistRule {
            shell_tool_name: "run_shell",
        };

        // ToolProfile の Exec allowlist は空（policy 側で allowlist 未設定）だが、
        // command_rules.txt 側の allowlist で許可されたコマンドは approval なしで Allow になることを確認する。
        let profile = profile_with_exec_allowlist("run_shell", &[], ToolMode::RequireApproval);
        let tool_ctx = ToolContext::new(None).with_command_allow_rules(vec![
            CommandAllowRule::Prefix("find".to_string()),
            CommandAllowRule::Prefix("grep".to_string()),
        ]);

        let tool_args = serde_json::json!({"command": "find . -maxdepth 1"});

        let verdict = rule
            .evaluate("run_shell", &tool_args, &profile, &tool_ctx, false)
            .expect("evaluate should succeed");

        match verdict {
            RuleVerdict::Verdict(PolicyVerdict::Allow { .. }) => {}
            _ => panic!("expected Allow when command is permitted by command_rules"),
        }
    }
}
