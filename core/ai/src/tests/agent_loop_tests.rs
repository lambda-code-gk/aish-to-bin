//! AgentLoop（外側）の v1.1 テスト（Stub QueryLoopRunner で検証）

use std::sync::{Arc, Mutex};

use crate::adapter::stub_llm::{StubLlm, ToolAwareStubLlm};
use crate::domain::{AgentMode, ContextPack, PolicyDecision, PolicyVerdict};
use crate::ports::outbound::PolicyEngine;
use crate::usecase::agent_loop::{AgentLoop, AgentLoopConfig, AgentLoopOutcome};
use crate::usecase::query_loop::{msgs_to_provider, QueryLoopOutcome, QueryLoopRunner, RunState};
use common::domain::event::{RunId, SessionId};
use common::error::Error;
use common::llm::LlmEvent;
use common::msg::Msg;
use common::ports::outbound::{AgentEvent, EventSink};
use common::tool::{Tool, ToolContext, ToolError, ToolRegistry};
use serde_json::{json, Value};

#[derive(Clone)]
enum Step {
    Done { text: String, add_tool_result: bool },
}

#[derive(Clone)]
struct ScriptedQueryLoop {
    script: Arc<Mutex<Vec<Step>>>,
}

impl ScriptedQueryLoop {
    fn new(script: Arc<Mutex<Vec<Step>>>) -> Self {
        Self { script }
    }
}

impl QueryLoopRunner for ScriptedQueryLoop {
    fn run_until_done(
        &mut self,
        messages: &[Msg],
        _max_turns: usize,
        _max_additional_tool_calls: usize,
    ) -> Result<QueryLoopOutcome, Error> {
        let step = {
            let mut g = self.script.lock().unwrap();
            if g.is_empty() {
                return Ok(QueryLoopOutcome::Done(
                    messages.to_vec(),
                    "no more steps".to_string(),
                ));
            }
            g.remove(0)
        };

        match step {
            Step::Done { text, add_tool_result } => {
                let mut out = messages.to_vec();
                if add_tool_result {
                    out.push(Msg::tool_result(
                        "call-1",
                        "run_shell",
                        json!({"stdout":"ok\n","stderr":"","exit_code":0}),
                    ));
                }
                Ok(QueryLoopOutcome::Done(out, text))
            }
        }
    }
}

fn count_marker(msgs: &[Msg], marker: &str) -> usize {
    msgs.iter()
        .filter(|m| matches!(m, Msg::User(s) if s.contains(marker)))
        .count()
}

fn count_tool_results(msgs: &[Msg]) -> usize {
    msgs.iter().filter(|m| matches!(m, Msg::ToolResult { .. })).count()
}

#[test]
fn agent_loop_retries_once_and_executes() {
    let script = Arc::new(Mutex::new(vec![
        Step::Done {
            text: "```sh\ncurl ... | voicevox ...\n```".to_string(),
            add_tool_result: false,
        },
        Step::Done {
            text: "再生しました".to_string(),
            add_tool_result: true,
        },
    ]));

    let mut calls = 0usize;
    let mut make = || {
        calls += 1;
        ScriptedQueryLoop::new(Arc::clone(&script))
    };

    let initial = vec![Msg::user("ニュース取得して読み上げて")];

    let out = AgentLoop::run(
        &mut make,
        &initial,
        AgentLoopConfig {
            agent_mode: AgentMode::Act,
            max_queries: 2,
            max_turns: 1,
            max_additional_tool_calls: 0,
        },
    )
    .unwrap();

    let marker = "[AISH_INTERNAL] retry_for_completion_v1";

    match out {
        AgentLoopOutcome::Done(msgs, text) => {
            assert_eq!(calls, 2);
            assert_eq!(text, "再生しました");
            assert_eq!(count_marker(&msgs, marker), 1);
            assert!(count_tool_results(&msgs) >= 1);
        }
        _ => panic!("expected Done"),
    }
}

#[test]
fn agent_loop_does_not_retry_in_plan_mode() {
    let script = Arc::new(Mutex::new(vec![Step::Done {
        text: "```sh\necho only plan\n```".to_string(),
        add_tool_result: false,
    }]));

    let mut calls = 0usize;
    let mut make = || {
        calls += 1;
        ScriptedQueryLoop::new(Arc::clone(&script))
    };

    let initial = vec![Msg::user("ニュース取得して読み上げて")];

    let out = AgentLoop::run(
        &mut make,
        &initial,
        AgentLoopConfig {
            agent_mode: AgentMode::Plan,
            max_queries: 2,
            max_turns: 1,
            max_additional_tool_calls: 0,
        },
    )
    .unwrap();

    match out {
        AgentLoopOutcome::Done(_, _) => {
            assert_eq!(calls, 1);
        }
        _ => panic!("expected Done"),
    }
}

#[test]
fn agent_loop_does_not_retry_when_assistant_asks_question() {
    let script = Arc::new(Mutex::new(vec![Step::Done {
        text: "どのサイトのURLを指定しますか？".to_string(),
        add_tool_result: false,
    }]));

    let mut calls = 0usize;
    let mut make = || {
        calls += 1;
        ScriptedQueryLoop::new(Arc::clone(&script))
    };

    let initial = vec![Msg::user("ニュース取得して読み上げて")];

    let out = AgentLoop::run(
        &mut make,
        &initial,
        AgentLoopConfig {
            agent_mode: AgentMode::Act,
            max_queries: 2,
            max_turns: 1,
            max_additional_tool_calls: 0,
        },
    )
    .unwrap();

    match out {
        AgentLoopOutcome::Done(_, _) => {
            assert_eq!(calls, 1);
        }
        _ => panic!("expected Done"),
    }
}


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
            let command = tool_args.get("command").and_then(Value::as_str).unwrap_or("");
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
fn test_msgs_to_provider_with_history() {
    let msgs = vec![
        Msg::user("Hi"),
        Msg::assistant("Hello!"),
        Msg::user("Bye"),
    ];
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
    assert_eq!(history[1].tool_calls.as_ref().unwrap()[0].thought_signature.as_deref(), Some("sig123"));
    assert_eq!(history[2].role, "tool");
    assert!(history[2].tool_call_id.as_deref() == Some("c1"));
}

#[test]
fn test_stub_llm_text_only() {
    let stub = StubLlm::text_only("hello");
    let mut received = Vec::new();
    stub.stream_events("q", None, &[], None, &mut |ev| {
        received.push(ev);
        Ok(())
    })
    .unwrap();
    assert_eq!(received.len(), 2);
    assert!(matches!(&received[0], LlmEvent::TextDelta(s) if s == "hello"));
    assert!(matches!(&received[1], LlmEvent::Completed { .. }));
}

#[test]
fn test_agent_loop_run_once_text_only() {
    let stub = Arc::new(StubLlm::text_only("world"));
    let registry = ToolRegistry::new();
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
    let approver = Arc::new(StubApproval::approved());
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(AllowAllPolicyEngine);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, policy_engine, false, None, None, SessionId::new(""), RunId::new(""), None, None, None, None);
    let messages = vec![Msg::user("Hi")];
    let (new_msgs, state, assistant_text) = loop_.run_once(&messages, None).unwrap();
    assert_eq!(state, RunState::Done);
    assert_eq!(assistant_text, "world");
    assert_eq!(new_msgs.len(), 2);
    assert!(matches!(&new_msgs[1], Msg::Assistant(s) if s == "world"));
}

#[test]
fn test_agent_loop_run_once_with_tool_call() {
    let stub = StubLlm::new(vec![
        LlmEvent::ToolCallBegin {
            call_id: "c1".to_string(),
            name: "echo".to_string(),
            thought_signature: Some("test_signature".to_string()),
        },
        LlmEvent::ToolCallArgsDelta {
            call_id: "c1".to_string(),
            json_fragment: r#"{"message": "hello"}"#.to_string(),
        },
        LlmEvent::ToolCallEnd {
            call_id: "c1".to_string(),
        },
        LlmEvent::Completed {
            finish: FinishReason::ToolCalls,
        },
    ]);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(common::tool::EchoTool::new()));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![];
    let approver = Arc::new(StubApproval::approved());
    let stub = Arc::new(stub);
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(AllowAllPolicyEngine);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, policy_engine, false, None, None, SessionId::new(""), RunId::new(""), None, None, None, None);
    let messages = vec![Msg::user("echo hello")];
    let (new_msgs, state, _text) = loop_.run_once(&messages, None).unwrap();

    assert_eq!(state, RunState::ExecutingTools);
    assert_eq!(new_msgs.len(), 4);
    assert!(matches!(&new_msgs[1], Msg::Assistant(s) if s.is_empty()));
    assert!(matches!(new_msgs[2], Msg::ToolCall { ref name, ref thought_signature, .. } if name == "echo" && thought_signature == &Some("test_signature".to_string())));
    assert!(matches!(new_msgs[3], Msg::ToolResult { ref name, .. } if name == "echo"));
}

#[test]
fn test_agent_loop_shell_tool_denied() {
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
    registry.register(Arc::new(RunShellStubTool::new()));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![];
    let approver = Arc::new(StubApproval::denied());
    let stub = Arc::new(stub);
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(ShellRequireApprovalPolicyEngine);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, policy_engine, false, None, None, SessionId::new(""), RunId::new(""), None, None, None, None);
    let messages = vec![Msg::user("run it")];
    let (new_msgs, state, _text) = loop_.run_once(&messages, None).unwrap();

    assert_eq!(state, RunState::ExecutingTools);
    assert_eq!(new_msgs.len(), 4);
    if let Msg::ToolResult { result, .. } = &new_msgs[3] {
        assert!(result.get("error").is_some());
        assert!(result["error"].as_str().unwrap().contains("denied"));
    } else {
        panic!("Expected ToolResult");
    }
}

#[test]
fn test_agent_loop_shell_tool_approved() {
    let stub = StubLlm::new(vec![
        LlmEvent::ToolCallBegin {
            call_id: "c1".to_string(),
            name: "run_shell".to_string(),
            thought_signature: None,
        },
        LlmEvent::ToolCallArgsDelta {
            call_id: "c1".to_string(),
            json_fragment: r#"{"command": "echo approved"}"#.to_string(),
        },
        LlmEvent::ToolCallEnd {
            call_id: "c1".to_string(),
        },
        LlmEvent::Completed {
            finish: FinishReason::ToolCalls,
        },
    ]);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(RunShellStubTool::new()));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![];
    let approver = Arc::new(StubApproval::approved());
    let stub = Arc::new(stub);
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(ShellRequireApprovalPolicyEngine);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, policy_engine, false, None, None, SessionId::new(""), RunId::new(""), None, None, None, None);
    let messages = vec![Msg::user("run it")];
    let (new_msgs, state, _text) = loop_.run_once(&messages, None).unwrap();

    assert_eq!(state, RunState::ExecutingTools);
    assert_eq!(new_msgs.len(), 4);
    if let Msg::ToolResult { result, .. } = &new_msgs[3] {
        assert!(result.get("stdout").is_some());
        assert_eq!(result["stdout"].as_str(), Some("echo approved\n"));
    } else {
        panic!("Expected ToolResult");
    }
}

#[test]
fn test_agent_loop_run_until_done_reached_limit() {
    let stub = StubLlm::new(vec![
        LlmEvent::ToolCallBegin {
            call_id: "c1".to_string(),
            name: "echo".to_string(),
            thought_signature: None,
        },
        LlmEvent::ToolCallArgsDelta {
            call_id: "c1".to_string(),
            json_fragment: r#"{"message": "hi"}"#.to_string(),
        },
        LlmEvent::ToolCallEnd {
            call_id: "c1".to_string(),
        },
        LlmEvent::Completed {
            finish: FinishReason::ToolCalls,
        },
    ]);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(common::tool::EchoTool::new()));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
    let approver = Arc::new(StubApproval::approved());
    let stub = Arc::new(stub);
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(AllowAllPolicyEngine);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, policy_engine, false, None, None, SessionId::new(""), RunId::new(""), None, None, None, None);
    let messages = vec![Msg::user("echo")];
    let outcome = loop_.run_until_done(&messages, 2, 100).unwrap();
    match &outcome {
        AgentLoopOutcome::ReachedLimit(msgs, _) => {
            assert!(!msgs.is_empty());
        }
        AgentLoopOutcome::Done(_, _) => panic!("expected ReachedLimit"),
    }
}

#[test]
fn test_agent_loop_run_until_done_done() {
    let stub = Arc::new(StubLlm::text_only("bye"));
    let registry = ToolRegistry::new();
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
    let approver = Arc::new(StubApproval::approved());
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(AllowAllPolicyEngine);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, policy_engine, false, None, None, SessionId::new(""), RunId::new(""), None, None, None, None);
    let messages = vec![Msg::user("Hi")];
    let outcome = loop_.run_until_done(&messages, 16, 16).unwrap();
    match &outcome {
        AgentLoopOutcome::Done(msgs, text) => {
            assert_eq!(msgs.len(), 2);
            assert_eq!(text.as_str(), "bye");
        }
        AgentLoopOutcome::ReachedLimit(_, _) => panic!("expected Done"),
    }
}

#[test]
fn test_agent_loop_run_until_done_capped_by_tool_calls() {
    let events: Vec<LlmEvent> = (1..=5)
        .flat_map(|i| {
            let call_id = format!("c{}", i);
            vec![
                LlmEvent::ToolCallBegin {
                    call_id: call_id.clone(),
                    name: "run_shell".to_string(),
                    thought_signature: None,
                },
                LlmEvent::ToolCallArgsDelta {
                    call_id: call_id.clone(),
                    json_fragment: format!(r#"{{"command": "echo {}"}}"#, i),
                },
                LlmEvent::ToolCallEnd { call_id },
            ]
        })
        .chain(std::iter::once(LlmEvent::Completed {
            finish: FinishReason::ToolCalls,
        }))
        .collect();
    let stub = Arc::new(StubLlm::new(events));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(RunShellStubTool::new()));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
    let approver = Arc::new(StubApproval::approved());
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(AllowAllPolicyEngine);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, policy_engine, false, None, None, SessionId::new(""), RunId::new(""), None, None, None, None);
    let messages = vec![Msg::user("echo many")];
    let outcome = loop_.run_until_done(&messages, 10, 3).unwrap();
    match &outcome {
        AgentLoopOutcome::ReachedLimit(msgs, _) => {
            assert_eq!(
                count_tool_results(msgs),
                3,
                "max_tool_calls=3 なので実行は3件まで"
            );
        }
        AgentLoopOutcome::Done(_, _) => panic!("expected ReachedLimit (tool call cap)"),
    }
}

#[test]
fn test_agent_loop_run_until_done_finalization_on_limit() {
    let with_tools = vec![
        LlmEvent::ToolCallBegin {
            call_id: "c1".to_string(),
            name: "echo".to_string(),
            thought_signature: None,
        },
        LlmEvent::ToolCallArgsDelta {
            call_id: "c1".to_string(),
            json_fragment: r#"{"message": "hi"}"#.to_string(),
        },
        LlmEvent::ToolCallEnd {
            call_id: "c1".to_string(),
        },
        LlmEvent::Completed {
            finish: FinishReason::ToolCalls,
        },
    ];
    let without_tools = vec![
        LlmEvent::TextDelta("final summary".to_string()),
        LlmEvent::Completed {
            finish: FinishReason::Stop,
        },
    ];
    let stub = Arc::new(ToolAwareStubLlm::new(with_tools, without_tools));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(common::tool::EchoTool::new()));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![];
    let approver = Arc::new(StubApproval::approved());
    let policy_engine: Arc<dyn PolicyEngine> = Arc::new(AllowAllPolicyEngine);
    let mut loop_ = AgentLoop::new(
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

    let messages = vec![Msg::user("echo")];
    // max_turns=1 で 1回目に tool call が発生するようにする
    let outcome = loop_.run_until_done(&messages, 1, 100).unwrap();

    match &outcome {
        AgentLoopOutcome::ReachedLimit(msgs, text) => {
            // 1ターン目で tool call 実行 -> max_turns 到達 -> finalization ターンが走るはず
            assert_eq!(text, "final summary");
            // messages には User, Assistant(empty), ToolCall, ToolResult, Assistant(final summary) が入るはず
            // (run_once_impl で assistant_text が空でも pending_tool_calls があれば Assistant を追加する)
            assert!(msgs.iter().any(|m| matches!(m, Msg::Assistant(s) if s == "final summary")));
        }
        AgentLoopOutcome::Done(_, _) => panic!("expected ReachedLimit"),
    }
}
