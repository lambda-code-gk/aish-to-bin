use super::policy_rules_tool::{ShellAllowlistRule, ToolModeRule};
use crate::domain::policy_rule::ToolPolicyRule;
use crate::domain::{PolicyVerdict, RuleVerdict, ToolCapability, ToolMode, ToolProfile};
use common::tool::ToolContext;

fn profile_with_exec_allowlist(tool_name: &str, allowlist: &[&str], mode: ToolMode) -> ToolProfile {
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
