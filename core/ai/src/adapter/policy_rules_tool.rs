use crate::domain::{
    PolicyDecision, PolicyVerdict, RuleVerdict, ToolCapability, ToolMode, ToolProfile,
};
use crate::domain::ToolPolicyRule;
use common::error::Error;
use common::tool::{is_command_allowed, ToolContext};

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

/// ToolProfile.mode に基づき Allow / RequireApproval / Deny を決めるルール
pub struct ToolModeRule;

impl ToolPolicyRule for ToolModeRule {
    fn name(&self) -> &'static str {
        "tool_mode"
    }

    fn evaluate(
        &self,
        tool_name: &str,
        _tool_args: &serde_json::Value,
        profile: &ToolProfile,
        tool_ctx: &ToolContext,
        non_interactive: bool,
    ) -> Result<RuleVerdict<ToolContext>, Error> {
        let subject = tool_name;
        match profile.mode {
            ToolMode::Allow => Ok(RuleVerdict::Verdict(PolicyVerdict::Allow {
                value: tool_ctx.clone(),
                decision: base_decision("tool", subject, "allowed", "tool_mode"),
            })),
            ToolMode::Deny => Ok(RuleVerdict::Verdict(PolicyVerdict::Deny {
                decision: base_decision("tool", subject, "blocked", "tool_mode"),
            })),
            ToolMode::RequireApproval => {
                if non_interactive {
                    let mut decision =
                        base_decision("tool", subject, "blocked", "approval_required_non_interactive");
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
                    Ok(RuleVerdict::Verdict(PolicyVerdict::RequireApproval {
                        value: tool_ctx.clone().with_allow_unsafe(true),
                        decision,
                        prompt: format!("tool {} requires approval", tool_name),
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
        let program = command.split_whitespace().next().unwrap_or("");
        let args_preview: String = command
            .split_whitespace()
            .skip(1)
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");

        // ToolProfile の Exec allowlist を優先し、なければ従来の ToolContext.command_allow_rules を見る
        let mut profile_allowlist: Vec<String> = Vec::new();
        for cap in &profile.capabilities {
            if let ToolCapability::Exec { allowlist } = cap {
                profile_allowlist.extend(allowlist.iter().cloned());
            }
        }

        let allowed_by_profile = if profile_allowlist.is_empty() {
            false
        } else {
            profile_allowlist
                .iter()
                .any(|prefix| program.starts_with(prefix))
        };

        let allowed_by_context = is_command_allowed(command, &tool_ctx.command_allow_rules);
        if allowed_by_profile || allowed_by_context {
            let mut decision = base_decision("tool", tool_name, "allowed", "shell_allowlist");
            decision.details = serde_json::json!({
                "program": program,
                "args_preview": args_preview,
            });
            return Ok(RuleVerdict::Verdict(PolicyVerdict::Allow {
                value: tool_ctx.clone(),
                decision,
            }));
        }

        if non_interactive {
            let mut decision =
                base_decision("tool", tool_name, "blocked", "shell_not_allowlisted_non_interactive");
            decision.details = serde_json::json!({
                "program": program,
                "args_preview": args_preview,
            });
            return Ok(RuleVerdict::Verdict(PolicyVerdict::Deny { decision }));
        }

        let mut decision =
            base_decision("tool", tool_name, "warn", "shell_approval_required");
        decision.details = serde_json::json!({
            "program": program,
            "args_preview": args_preview,
        });
        Ok(RuleVerdict::Verdict(PolicyVerdict::RequireApproval {
            value: tool_ctx.clone().with_allow_unsafe(true),
            decision,
            prompt: command.to_string(),
        }))
    }
}

