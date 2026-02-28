use crate::domain::ToolPolicyRule;
use crate::domain::{
    PolicyDecision, PolicyVerdict, RuleVerdict, ToolCapability, ToolMode, ToolProfile,
};
use common::error::Error;
use common::tool::ToolContext;

const TOOL_SUMMARY_MAX_CHARS: usize = 200;

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

fn tool_summary_preview(tool_name: &str, tool_args: &serde_json::Value) -> String {
    let args_str = serde_json::to_string(tool_args).unwrap_or_else(|_| "{}".to_string());
    let summary = format!("{} {}", tool_name, args_str);
    if summary.len() <= TOOL_SUMMARY_MAX_CHARS {
        summary
    } else {
        format!("{}...(truncated)", &summary[..TOOL_SUMMARY_MAX_CHARS])
    }
}

/// ToolProfile.mode に基づき Allow / RequireApproval / Deny を決めるルール
pub struct ToolModeRule;

impl ToolPolicyRule for ToolModeRule {
    fn name(&self) -> &'static str {
        "tool_mode"
    }

    fn evaluate(
        &self,
        tool_name: &str,
        tool_args: &serde_json::Value,
        profile: &ToolProfile,
        tool_ctx: &ToolContext,
        non_interactive: bool,
    ) -> Result<RuleVerdict<ToolContext>, Error> {
        let subject = tool_name;
        match profile.mode {
            ToolMode::Allow => Ok(RuleVerdict::Verdict(PolicyVerdict::Allow {
                value: tool_ctx.clone(),
                decision: base_decision("tool", subject, "allowed", "tool_mode_allow"),
            })),
            ToolMode::Deny => Ok(RuleVerdict::Verdict(PolicyVerdict::Deny {
                decision: base_decision("tool", subject, "blocked", "tool_mode_deny"),
            })),
            ToolMode::RequireApproval => {
                if non_interactive {
                    let mut decision = base_decision(
                        "tool",
                        subject,
                        "blocked",
                        "approval_required_non_interactive",
                    );
                    decision.details = serde_json::json!({
                        "mode": "require_approval",
                    });
                    Ok(RuleVerdict::Verdict(PolicyVerdict::Deny { decision }))
                } else {
                    let mut decision =
                        base_decision("tool", subject, "warn", "tool_mode_require_approval");
                    decision.details = serde_json::json!({
                        "mode": "require_approval",
                    });
                    let prompt = tool_summary_preview(tool_name, tool_args);
                    Ok(RuleVerdict::Verdict(PolicyVerdict::RequireApproval {
                        value: tool_ctx.clone().with_allow_unsafe(true),
                        decision,
                        prompt,
                    }))
                }
            }
        }
    }
}

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
        let first_token = command.split_whitespace().next().unwrap_or("");

        // profile.capabilities から Exec allowlist を抽出（無ければ empty）
        let mut profile_allowlist: Vec<String> = Vec::new();
        for cap in &profile.capabilities {
            if let ToolCapability::Exec { allowlist } = cap {
                profile_allowlist.extend(allowlist.iter().cloned());
            }
        }

        // prefix or first-token match
        let allowed = if profile_allowlist.is_empty() {
            false
        } else {
            profile_allowlist
                .iter()
                .any(|prefix| first_token.starts_with(prefix) || first_token == prefix)
        };

        if allowed {
            let mut decision = base_decision("tool", tool_name, "allowed", "shell_allowlist");
            decision.details = serde_json::json!({
                "command": if command.len() <= TOOL_SUMMARY_MAX_CHARS { command.to_string() } else { format!("{}...(truncated)", &command[..TOOL_SUMMARY_MAX_CHARS]) },
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
                "command": if command.len() <= TOOL_SUMMARY_MAX_CHARS { command.to_string() } else { format!("{}...(truncated)", &command[..TOOL_SUMMARY_MAX_CHARS]) },
            });
            return Ok(RuleVerdict::Verdict(PolicyVerdict::Deny { decision }));
        }

        let mut decision = base_decision("tool", tool_name, "warn", "shell_approval_required");
        decision.details = serde_json::json!({
            "command": if command.len() <= TOOL_SUMMARY_MAX_CHARS { command.to_string() } else { format!("{}...(truncated)", &command[..TOOL_SUMMARY_MAX_CHARS]) },
        });
        Ok(RuleVerdict::Verdict(PolicyVerdict::RequireApproval {
            value: tool_ctx.clone().with_allow_unsafe(true),
            decision,
            prompt: command.to_string(),
        }))
    }
}
