//! StdPolicyExplainProvider の explain() が resolved / ルール名 / examples を返すことを検証

use crate::adapter::StdPolicyExplainProvider;
use crate::domain::{PolicyDecision, PolicyExplainExample};
use crate::ports::outbound::PolicyExplainProvider;

#[test]
fn test_explain_returns_v_resolved_rules_and_examples() {
    let resolved = serde_json::json!({
        "egress": { "hard_cap_chars": 200_000 },
        "tools": { "run_shell": { "mode": "require_approval" } },
    });
    let egress_rule_names = vec![
        "egress_budget_hard_cap".to_string(),
        "egress_sensitive".to_string(),
    ];
    let tool_rule_names = vec!["shell_allowlist".to_string(), "tool_mode".to_string()];
    let examples = vec![
        PolicyExplainExample {
            title: "example 1".to_string(),
            input: serde_json::json!({"tool": "run_shell"}),
            outcome: serde_json::json!({"status": "allowed"}),
            decisions: vec![PolicyDecision {
                v: 1,
                scope: "tool".to_string(),
                subject: "run_shell".to_string(),
                status: "allowed".to_string(),
                reason: "shell_allowlist".to_string(),
                details: serde_json::json!({}),
            }],
        },
        PolicyExplainExample {
            title: "example 2".to_string(),
            input: serde_json::json!({"scope": "egress"}),
            outcome: serde_json::json!({"status": "mask_or_deny"}),
            decisions: vec![],
        },
    ];
    let provider = StdPolicyExplainProvider::new(
        resolved.clone(),
        egress_rule_names.clone(),
        tool_rule_names.clone(),
        examples.clone(),
    );
    let info = provider.explain().expect("explain must succeed");
    assert_eq!(info.v, 1);
    assert_eq!(info.resolved, resolved);
    assert_eq!(info.egress_rules, egress_rule_names);
    assert_eq!(info.tool_rules, tool_rule_names);
    assert!(
        info.examples.len() >= 2,
        "examples must have at least 2 items"
    );
    assert_eq!(info.examples[0].title, "example 1");
    assert_eq!(info.examples[1].title, "example 2");
}
