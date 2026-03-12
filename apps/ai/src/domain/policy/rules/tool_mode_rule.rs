//! 責務: ToolProfile.mode に基づき、tool 呼び出しの Allow / RequireApproval / Deny を決める純粋な Rule 実装。I/O や表示仕様は知らない。

use crate::domain::{
    tool_summary_preview, PolicyVerdict, RuleVerdict, ToolMode, ToolPolicyRule, ToolProfile,
};
use common::error::Error;
use common::tool::ToolContext;

use super::base_decision;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PolicyVerdict, RuleVerdict, ToolCapability, ToolPolicyRule};
    use common::tool::ToolContext;

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
    fn replace_file_prompt_shows_path_and_change_preview() {
        let rule = ToolModeRule;
        let profile = profile_with_exec_allowlist("replace_file", &[], ToolMode::RequireApproval);
        let tool_ctx = ToolContext::new(None);

        let tool_args = serde_json::json!({
            "path": "/workspace/src/main.rs",
            "old_block": "fn old() { println!(\"old\"); }",
            "new_block": "fn new() { println!(\"new\"); }",
        });

        let verdict = rule
            .evaluate("replace_file", &tool_args, &profile, &tool_ctx, false)
            .expect("evaluate should succeed");

        match verdict {
            RuleVerdict::Verdict(PolicyVerdict::RequireApproval { prompt, .. }) => {
                // パスと old/new の概要が含まれていること
                assert!(prompt.contains("replace_file"));
                assert!(prompt.contains("/workspace/src/main.rs"));
                assert!(prompt.contains("old:["));
                assert!(prompt.contains("-> new:["));
                // 長くなりすぎていないこと（上限+suffix 程度）
                assert!(prompt.chars().count() <= 200 + "...(truncated)".chars().count());
            }
            _ => panic!("expected RequireApproval for replace_file"),
        }
    }

    #[test]
    fn tool_summary_preview_panics_on_utf8_when_truncating_by_bytes() {
        // 目的:
        // - 以前の実装(summary[..200])だと必ずpanicする入力を作る
        // - 今の実装(truncate_chars)だとpanicしない
        //
        // summary = format!("{} {}", tool_name, args_json)
        // tool_name を ASCII で長さ 199 にしておくと、summary[..200] は
        // "<199 bytes of a> + ' '" までで安全。
        // その直後(201バイト目)に UTF-8 3バイト文字(例: "あ")が来るように
        // args_json の先頭を調整する。
        //
        // args_json は "{\"text\":\"<value>\"}" になるので、value の先頭を
        // "あ" にすれば、summary の 201バイト目が UTF-8 文字の途中になり、
        // バイトスライス実装ならpanicする。

        let tool_name = "a".repeat(199);
        let tool_args = serde_json::json!({
            "text": format!("{}{}", "あ", "x".repeat(400))
        });

        let rule = ToolModeRule;
        let profile = profile_with_exec_allowlist(&tool_name, &[], ToolMode::RequireApproval);
        let tool_ctx = ToolContext::new(None);

        // ここで panic しないことが、この修正の主目的。
        let verdict = rule
            .evaluate(&tool_name, &tool_args, &profile, &tool_ctx, false)
            .expect("evaluate should succeed");

        match verdict {
            RuleVerdict::Verdict(PolicyVerdict::RequireApproval { prompt, .. }) => {
                assert!(prompt.ends_with("...(truncated)"));
                assert!(prompt.chars().count() <= 200 + "...(truncated)".chars().count());
            }
            _ => panic!("expected RequireApproval"),
        }
    }
}

