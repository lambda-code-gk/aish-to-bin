//! QueryLoop × PolicyEngine 統合テスト: Deny verdict が ToolError になること（元 AgentLoop 統合テスト）

use std::sync::Arc;

use common::domain::event::{RunId, SessionId};
use common::error::Error;
use common::llm::events::{FinishReason, LlmEvent};
use common::msg::Msg;
use common::sink::{AgentEvent, EventSink};
use common::tool::{Tool, ToolContext, ToolError, ToolRegistry};
use serde_json::Value;

use crate::adapter::llm::stub_llm::StubLlm;
use crate::domain::approval::StubApproval;
use crate::domain::{ContextPack, PolicyDecision, PolicyVerdict};
use crate::ports::outbound::PolicyEngine;
use crate::usecase::query_loop::{QueryLoop, RunState};

struct RunShellStubTool;
impl Tool for RunShellStubTool {
    fn name(&self) -> &'static str {
        "run_shell"
    }
    fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        let command = args.get("command").and_then(Value::as_str).unwrap_or("");
        Ok(serde_json::json!({"stdout": format!("{}\n", command), "stderr": "", "exit_code": 0}))
    }
}

/// すべてのツール呼び出しを Deny する PolicyEngine
struct DenyAllToolsPolicyEngine;
impl PolicyEngine for DenyAllToolsPolicyEngine {
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
        _tool_ctx: &ToolContext,
        _non_interactive: bool,
    ) -> Result<PolicyVerdict<ToolContext>, Error> {
        Ok(PolicyVerdict::Deny {
            decision: PolicyDecision {
                v: 1,
                scope: "tool".to_string(),
                subject: tool_name.to_string(),
                status: "blocked".to_string(),
                reason: "test_deny".to_string(),
                details: serde_json::json!({}),
            },
        })
    }
}

/// 何も出力しない EventSink
struct StubEventSink;
impl EventSink for StubEventSink {
    fn on_event(&mut self, _ev: &AgentEvent) -> Result<(), Error> {
        Ok(())
    }
    fn on_end(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

#[test]
fn test_deny_policy_emits_tool_error() {
    let stub = StubLlm::new(vec![
        LlmEvent::ToolCallBegin {
            call_id: "c1".to_string(),
            name: "run_shell".to_string(),
            thought_signature: None,
        },
        LlmEvent::ToolCallArgsDelta {
            call_id: "c1".to_string(),
            json_fragment: r#"{"command": "echo hello"}"#.to_string(),
        },
        LlmEvent::ToolCallEnd {
            call_id: "c1".to_string(),
        },
        LlmEvent::Completed {
            finish: FinishReason::ToolCalls,
        },
    ]);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(RunShellStubTool));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink)];
    let approver = Arc::new(StubApproval::approved());
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(DenyAllToolsPolicyEngine);
    let stub = Arc::new(stub);
    let mut loop_ = QueryLoop::new(
        stub,
        registry,
        ctx,
        sinks,
        approver,
        policy_engine,
        false,
        None,
        None,
        SessionId::new(""),
        RunId::new(""),
        None,
        None,
        None,
        None,
    );
    let messages = vec![Msg::user("run it")];
    let (new_msgs, state, _text) = loop_.run_once(&messages, None).unwrap();

    assert_eq!(state, RunState::ExecutingTools);
    // ToolCall + ToolResult(error) が追加されるはず
    if let Msg::ToolResult { result, .. } = &new_msgs[3] {
        let err = result.get("error").and_then(Value::as_str).unwrap_or("");
        assert!(
            err.contains("blocked by policy"),
            "expected blocked-by-policy error, got: {}",
            err
        );
    } else {
        panic!("expected ToolResult at index 3");
    }
}

#[test]
fn test_non_interactive_require_approval_is_denied() {
    /// shell ツールに RequireApproval を返す PolicyEngine
    struct RequireApprovalPolicyEngine;
    impl PolicyEngine for RequireApprovalPolicyEngine {
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
            _tool_name: &str,
            _tool_args: &Value,
            tool_ctx: &ToolContext,
            _non_interactive: bool,
        ) -> Result<PolicyVerdict<ToolContext>, Error> {
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
                prompt: "dangerous command".to_string(),
            })
        }
    }

    let stub = StubLlm::new(vec![
        LlmEvent::ToolCallBegin {
            call_id: "c1".to_string(),
            name: "run_shell".to_string(),
            thought_signature: None,
        },
        LlmEvent::ToolCallArgsDelta {
            call_id: "c1".to_string(),
            json_fragment: r#"{"command": "rm -rf /"}"#.to_string(),
        },
        LlmEvent::ToolCallEnd {
            call_id: "c1".to_string(),
        },
        LlmEvent::Completed {
            finish: FinishReason::ToolCalls,
        },
    ]);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(RunShellStubTool));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![];
    let approver = Arc::new(StubApproval::approved());
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(RequireApprovalPolicyEngine);
    let stub = Arc::new(stub);
    let mut loop_ = QueryLoop::new(
        stub,
        registry,
        ctx,
        sinks,
        approver,
        policy_engine,
        true, // non_interactive
        None,
        None,
        SessionId::new(""),
        RunId::new(""),
        None,
        None,
        None,
        None,
    );
    let messages = vec![Msg::user("run it")];
    let (new_msgs, state, _text) = loop_.run_once(&messages, None).unwrap();

    assert_eq!(state, RunState::ExecutingTools);
    if let Msg::ToolResult { result, .. } = &new_msgs[3] {
        let err = result.get("error").and_then(Value::as_str).unwrap_or("");
        assert!(
            err.contains("non-interactive"),
            "expected non-interactive denial, got: {}",
            err
        );
    } else {
        panic!("expected ToolResult at index 3");
    }
}
