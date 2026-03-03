//! QueryLoop の単体テスト（StubLlm は adapter のテスト用実装を使用）

use std::sync::Arc;

use common::domain::event::{RunId, SessionId};
use common::error::Error;
use common::llm::events::{FinishReason, LlmEvent};
use common::msg::Msg;
use common::sink::{AgentEvent, EventSink};
use common::tool::{Tool, ToolContext, ToolError, ToolRegistry};
use serde_json::Value;

use crate::adapter::stub_llm::{StubLlm, ToolAwareStubLlm};
use crate::domain::approval::StubApproval;
use crate::domain::{ContextPack, PolicyDecision, PolicyVerdict};
use crate::ports::outbound::{LlmEventStream, PolicyEngine};
use crate::usecase::query_loop::{
    count_tool_results, msgs_to_provider, QueryLoop, QueryLoopOutcome, RunState,
};

/// 以降のテストは元の agent_loop_tests.rs から移植されたものです。
/// 実装は QueryLoop に移ったため、型名のみ変更しています。

/// テスト用: 何も出力しない EventSink
struct StubEventSink;
impl StubEventSink {
    fn new() -> Self {
        Self
    }
}
impl EventSink for StubEventSink {
    fn on_event(&mut self, _ev: &AgentEvent) -> Result<(), common::error::Error> {
        Ok(())
    }
    fn on_end(&mut self) -> Result<(), common::error::Error> {
        Ok(())
    }
}

/// テスト用: すべて Allow を返す PolicyEngine
struct AllowAllPolicyEngine;
impl PolicyEngine for AllowAllPolicyEngine {
    fn evaluate_egress_context_pack(
        &self,
        pack: &ContextPack,
        _non_interactive: bool,
    ) -> Result<PolicyVerdict<ContextPack>, Error> {
        Ok(PolicyVerdict::Allow {
            value: pack.clone(),
            decision: PolicyDecision {
                v: 1,
                scope: "egress".to_string(),
                subject: "context_pack".to_string(),
                status: "allowed".to_string(),
                reason: "stub".to_string(),
                details: serde_json::json!({}),
            },
        })
    }
    fn evaluate_tool_call(
        &self,
        tool_name: &str,
        _tool_args: &Value,
        tool_ctx: &ToolContext,
        _non_interactive: bool,
    ) -> Result<PolicyVerdict<ToolContext>, Error> {
        Ok(PolicyVerdict::Allow {
            value: tool_ctx.clone(),
            decision: PolicyDecision {
                v: 1,
                scope: "tool".to_string(),
                subject: tool_name.to_string(),
                status: "allowed".to_string(),
                reason: "stub".to_string(),
                details: serde_json::json!({}),
            },
        })
    }
}

/// テスト用: shell ツールに RequireApproval を返す PolicyEngine
struct ShellRequireApprovalPolicyEngine;
impl PolicyEngine for ShellRequireApprovalPolicyEngine {
    fn evaluate_egress_context_pack(
        &self,
        pack: &ContextPack,
        _non_interactive: bool,
    ) -> Result<PolicyVerdict<ContextPack>, Error> {
        Ok(PolicyVerdict::Allow {
            value: pack.clone(),
            decision: PolicyDecision {
                v: 1,
                scope: "egress".to_string(),
                subject: "context_pack".to_string(),
                status: "allowed".to_string(),
                reason: "stub".to_string(),
                details: serde_json::json!({}),
            },
        })
    }
    fn evaluate_tool_call(
        &self,
        tool_name: &str,
        tool_args: &Value,
        tool_ctx: &ToolContext,
        _non_interactive: bool,
    ) -> Result<PolicyVerdict<ToolContext>, Error> {
        if tool_name == "run_shell" {
            let command = tool_args
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("");
            Ok(PolicyVerdict::RequireApproval {
                value: tool_ctx.clone().with_allow_unsafe(true),
                decision: PolicyDecision {
                    v: 1,
                    scope: "tool".to_string(),
                    subject: "run_shell".to_string(),
                    status: "warn".to_string(),
                    reason: "approval_required".to_string(),
                    details: serde_json::json!({}),
                },
                prompt: command.to_string(),
            })
        } else {
            Ok(PolicyVerdict::Allow {
                value: tool_ctx.clone(),
                decision: PolicyDecision {
                    v: 1,
                    scope: "tool".to_string(),
                    subject: tool_name.to_string(),
                    status: "allowed".to_string(),
                    reason: "stub".to_string(),
                    details: serde_json::json!({}),
                },
            })
        }
    }
}

/// テスト用: name "run_shell" の Tool
struct RunShellStubTool;
impl RunShellStubTool {
    fn new() -> Self {
        Self
    }
}
impl Tool for RunShellStubTool {
    fn name(&self) -> &'static str {
        "run_shell"
    }
    fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        let command = args.get("command").and_then(Value::as_str).unwrap_or("");
        Ok(serde_json::json!({
            "stdout": format!("{}\n", command),
            "stderr": "",
            "exit_code": 0
        }))
    }
}

#[test]
fn test_msgs_to_provider_simple() {
    let msgs = vec![Msg::user("Hello")];
    let (sys, query, history) = msgs_to_provider(&msgs);
    assert!(sys.is_none());
    assert_eq!(query, "Hello");
    assert!(history.is_empty());
}

#[test]
fn test_msgs_to_provider_concatenates_multiple_system_messages() {
    let msgs = vec![
        Msg::system("SYS1"),
        Msg::user("hi"),
        Msg::system("[AISH_INTERNAL] retry_for_completion_v1"),
        Msg::user("go"),
    ];
    let (sys, query, _history) = msgs_to_provider(&msgs);
    let sys = sys.expect("system should exist");
    assert!(sys.contains("SYS1"));
    assert!(sys.contains("[AISH_INTERNAL] retry_for_completion_v1"));
    assert_eq!(query, "go");
}

#[test]
fn test_msgs_to_provider_with_history() {
    let msgs = vec![Msg::user("Hi"), Msg::assistant("Hello!"), Msg::user("Bye")];
    let (_sys, query, history) = msgs_to_provider(&msgs);
    assert_eq!(query, "Bye");
    assert_eq!(history.len(), 2);
}

#[test]
fn test_msgs_to_provider_with_tool_call_and_result() {
    let msgs = vec![
        Msg::user("run it"),
        Msg::assistant("ok"),
        Msg::ToolCall {
            call_id: "c1".to_string(),
            name: "run".to_string(),
            args: serde_json::json!({"cmd": "ls"}),
            thought_signature: Some("sig123".to_string()),
        },
        Msg::ToolResult {
            call_id: "c1".to_string(),
            name: "run".to_string(),
            result: serde_json::json!({"ok": true}),
        },
    ];
    let (_sys, query, history) = msgs_to_provider(&msgs);
    assert_eq!(query, "");
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].role, "user");
    assert_eq!(history[1].role, "assistant");
    assert!(history[1].tool_calls.is_some());
    assert_eq!(history[1].tool_calls.as_ref().unwrap().len(), 1);
    assert_eq!(
        history[1].tool_calls.as_ref().unwrap()[0]
            .thought_signature
            .as_deref(),
        Some("sig123")
    );
    assert_eq!(history[2].role, "tool");
    assert!(history[2].tool_call_id.as_deref() == Some("c1"));
}

// StubLlm + QueryLoop の単体テスト以降は、もとの AgentLoop テストに準拠しつつ
// 型名のみ QueryLoop に変更して追加していく前提。
