//! QueryLoop: イベント解釈器 + 状態機械
//!
//! 直列の transaction script をやめ、RunState で遷移する。
//! LLM から ToolCallEnd が来たら tool 実行フェーズへ遷移し、結果を messages に注入する。

use crate::domain::{hash64, ContextAttachment, ContextSource, EventEnvelope, PolicyVerdict};
use crate::ports::outbound::{
    Approval, ContextArtifactStore, EventAppender, InterruptChecker, LlmEventStream, PolicyEngine,
    ToolApproval,
};
use common::domain::event::{Event, RunId, SessionId};
use common::domain::{EventEnvelopeWithoutSeq, SessionDir};
use common::error::Error;
use common::event_hub::EventHubHandle;
use common::llm::events::{FinishReason, LlmEvent};
use common::llm::provider::Message;
use common::msg::Msg;
use common::ports::outbound::Clock;
use common::sink::{AgentEvent, EventSink};
use common::tool::{ToolContext, ToolRegistry};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// メッセージ列に含まれるツール結果（実行済みツール呼び出し）の数を返す
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn count_tool_results(messages: &[Msg]) -> usize {
    messages
        .iter()
        .filter(|m| matches!(m, Msg::ToolResult { .. }))
        .count()
}

/// 実行状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// LLM ストリーム受信中
    StreamingModel,
    /// ツール実行中
    ExecutingTools,
    /// 正常終了
    Done,
    /// エラー終了（将来の LlmEvent::Failed 処理で使用）
    #[allow(dead_code)]
    Error,
}

/// クエリループの終了結果（Done と上限到達を区別する）
#[derive(Debug, Clone)]
pub enum QueryLoopOutcome {
    /// 正常終了（LLM が Stop 等で終了）
    Done(Vec<Msg>, String),
    /// 最大ターン数に達したが会話は継続可能
    ReachedLimit(Vec<Msg>, String),
}

/// QueryLoop の実行を抽象化するトレイト（テスト差し込み用）
pub(crate) trait QueryLoopRunner {
    fn run_until_done(
        &mut self,
        initial_messages: &[Msg],
        max_turns: usize,
        max_additional_tool_calls: usize,
    ) -> Result<QueryLoopOutcome, Error>;
}

/// Vec<Msg> をドライバ用 (system_instruction, query, history) に変換
/// ToolCall/ToolResult は Assistant(content, tool_calls) と Tool(call_id, result) に変換
pub fn msgs_to_provider(msgs: &[Msg]) -> (Option<String>, String, Vec<Message>) {
    let mut system: Option<String> = None;
    let mut list: Vec<Message> = Vec::new();
    let mut last_user: Option<String> = None;
    let mut pending_assistant: Option<String> = None;
    let mut pending_tool_calls: Vec<(String, String, Value, Option<String>)> = Vec::new();

    fn flush_assistant_with_tool_calls(
        list: &mut Vec<Message>,
        pending_assistant: &mut Option<String>,
        pending_tool_calls: &mut Vec<(String, String, Value, Option<String>)>,
    ) {
        if pending_assistant.is_some() || !pending_tool_calls.is_empty() {
            let content = pending_assistant.take().unwrap_or_default();
            let tool_calls = std::mem::take(pending_tool_calls);
            list.push(Message::assistant_with_tool_calls(content, tool_calls));
        }
    }

    for m in msgs {
        match m {
            Msg::System(s) => match &mut system {
                None => system = Some(s.clone()),
                Some(existing) => {
                    if !existing.is_empty() {
                        existing.push_str("\n\n");
                    }
                    existing.push_str(s);
                }
            },
            Msg::User(s) => {
                flush_assistant_with_tool_calls(
                    &mut list,
                    &mut pending_assistant,
                    &mut pending_tool_calls,
                );
                last_user = Some(s.clone());
                list.push(Message::user(s));
            }
            Msg::Assistant(s) => {
                flush_assistant_with_tool_calls(
                    &mut list,
                    &mut pending_assistant,
                    &mut pending_tool_calls,
                );
                pending_assistant = Some(s.clone());
            }
            Msg::ToolCall {
                call_id,
                name,
                args,
                thought_signature,
            } => {
                if pending_assistant.is_none() {
                    pending_assistant = Some(String::new());
                }
                pending_tool_calls.push((
                    call_id.clone(),
                    name.clone(),
                    args.clone(),
                    thought_signature.clone(),
                ));
            }
            Msg::ToolResult {
                call_id,
                name,
                result,
            } => {
                flush_assistant_with_tool_calls(
                    &mut list,
                    &mut pending_assistant,
                    &mut pending_tool_calls,
                );
                let content = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                list.push(Message::tool_result(call_id.clone(), name.clone(), content));
            }
        }
    }
    flush_assistant_with_tool_calls(&mut list, &mut pending_assistant, &mut pending_tool_calls);

    // 最後が User なら query に分離、そうでなければ継続呼び出しなので query="" で history を全文
    let last_is_user = msgs
        .last()
        .map(|m| matches!(m, Msg::User(_)))
        .unwrap_or(false);
    let (query, history) = if last_is_user && last_user.is_some() {
        let q = last_user.as_ref().map(String::clone).unwrap_or_default();
        let h = list
            .iter()
            .take(list.len().saturating_sub(1))
            .cloned()
            .collect();
        (q, h)
    } else {
        (String::new(), list)
    };
    (system, query, history)
}

/// ストリーム中に蓄積するツール呼び出し（call_id -> (name, args_json_fragments, thought_signature)）
#[allow(dead_code)] // completed は将来の複数ツール蓄積用
struct ToolCallAccumulator {
    current_id: Option<String>,
    current_name: Option<String>,
    current_thought_signature: Option<String>,
    args_fragments: String,
    completed: Vec<(String, String, Value, Option<String>)>,
}

impl ToolCallAccumulator {
    fn new() -> Self {
        Self {
            current_id: None,
            current_name: None,
            current_thought_signature: None,
            args_fragments: String::new(),
            completed: Vec::new(),
        }
    }

    fn on_begin(&mut self, call_id: String, name: String, thought_signature: Option<String>) {
        self.current_id = Some(call_id);
        self.current_name = Some(name);
        self.current_thought_signature = thought_signature;
        self.args_fragments.clear();
    }

    fn on_args_delta(&mut self, fragment: String) {
        self.args_fragments.push_str(&fragment);
    }

    fn on_end(
        &mut self,
        call_id: String,
    ) -> Result<Option<(String, String, Value, Option<String>)>, Error> {
        let name = self.current_name.take().unwrap_or_default();
        let thought_signature = self.current_thought_signature.take();
        self.current_id = None;
        let args = if self.args_fragments.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&self.args_fragments)
                .map_err(|e| Error::json(format!("Invalid tool args JSON: {}", e)))?
        };
        self.args_fragments.clear();
        Ok(Some((call_id, name, args, thought_signature)))
    }
}

/// QueryLoop: 状態機械で LlmEvent を処理し、Sink に流す
pub struct QueryLoop {
    stream: Arc<dyn LlmEventStream>,
    tool_registry: ToolRegistry,
    tool_context: ToolContext,
    sinks: Vec<Box<dyn EventSink>>,
    approver: Arc<dyn ToolApproval>,
    policy_engine: Arc<dyn PolicyEngine>,
    non_interactive: bool,
    /// Ctrl+C 等の割り込み検知。Some のときストリームコールバック内でチェックする
    interrupt_checker: Option<Arc<dyn InterruptChecker>>,
    /// transcript / HumanLog 用。Some のとき provider.* / policy.* を emit
    event_hub: Option<EventHubHandle>,
    session_id: SessionId,
    run_id: RunId,
    /// events.ndjson 追記用（session ありのときのみ）
    session_dir: Option<SessionDir>,
    event_appender: Option<Arc<dyn EventAppender>>,
    clock: Option<Arc<dyn Clock>>,
    /// tool call の args/result を artifacts に逃がす（session ありのときのみ）
    artifact_store: Option<Arc<dyn ContextArtifactStore>>,
}

impl QueryLoop {
    pub fn new(
        stream: Arc<dyn LlmEventStream>,
        tool_registry: ToolRegistry,
        tool_context: ToolContext,
        sinks: Vec<Box<dyn EventSink>>,
        approver: Arc<dyn ToolApproval>,
        policy_engine: Arc<dyn PolicyEngine>,
        non_interactive: bool,
        interrupt_checker: Option<Arc<dyn InterruptChecker>>,
        event_hub: Option<EventHubHandle>,
        session_id: SessionId,
        run_id: RunId,
        session_dir: Option<SessionDir>,
        event_appender: Option<Arc<dyn EventAppender>>,
        clock: Option<Arc<dyn Clock>>,
        artifact_store: Option<Arc<dyn ContextArtifactStore>>,
    ) -> Self {
        Self {
            stream,
            tool_registry,
            tool_context,
            sinks,
            approver,
            policy_engine,
            non_interactive,
            interrupt_checker,
            event_hub,
            session_id,
            run_id,
            session_dir,
            event_appender,
            clock,
            artifact_store,
        }
    }

    fn append_event(&self, kind: &str, payload: serde_json::Value) -> Result<(), Error> {
        let (dir, appender, clock) = match (
            self.session_dir.as_ref(),
            self.event_appender.as_ref(),
            self.clock.as_ref(),
        ) {
            (Some(d), Some(a), Some(c)) => (d, a, c),
            _ => return Ok(()),
        };
        let ts_ms = clock.now_ms() as i64;
        let envelope = EventEnvelopeWithoutSeq {
            v: EventEnvelope::SCHEMA_VERSION,
            ts_ms,
            session_id: self.session_id.0.clone(),
            run_id: Some(self.run_id.0.clone()),
            kind: kind.to_string(),
            payload,
        };
        appender.append(dir, envelope).map(|_| ())
    }

    fn append_policy_evaluated(&self, payload: serde_json::Value) -> Result<(), Error> {
        self.append_event("policy.evaluated", payload)
    }

    fn preview_str(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            return s.to_string();
        }
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if i >= max_chars.saturating_sub(3) {
                break;
            }
            out.push(ch);
        }
        out.push_str("...");
        out
    }

    fn maybe_store_text_artifact(
        &self,
        kind: &str,
        title: &str,
        content_type: &str,
        content: &str,
    ) -> Result<Option<ContextAttachment>, Error> {
        let (dir, store) = match (self.session_dir.as_ref(), self.artifact_store.as_ref()) {
            (Some(d), Some(s)) => (d, s),
            _ => return Ok(None),
        };
        let att = ContextAttachment {
            kind: kind.to_string(),
            title: title.to_string(),
            content_type: content_type.to_string(),
            content: Some(content.to_string()),
            artifact_rel_path: None,
            bytes: content.len() as u64,
            hash64: hash64(content),
            source: Some(ContextSource {
                kind: "tool".to_string(),
                ref_id: title.to_string(),
            }),
        };
        let stored = store.store(dir, &self.run_id, &[att])?;
        Ok(stored.into_iter().next())
    }

    fn tool_payload_with_artifact(
        &self,
        field: &str,
        kind: &str,
        title: &str,
        content_type: &str,
        value: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        let s = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
        let bytes = s.as_bytes().len();
        if bytes <= EventEnvelope::RECOMMENDED_MAX_PAYLOAD_BYTES {
            let mut m = serde_json::Map::new();
            m.insert(field.to_string(), value.clone());
            m.insert(format!("{}_bytes", field), serde_json::json!(bytes));
            m.insert(format!("{}_hash64", field), serde_json::json!(hash64(&s)));
            return Ok(serde_json::Value::Object(m));
        }
        let stored = self.maybe_store_text_artifact(kind, title, content_type, &s)?;
        let preview = Self::preview_str(&s, 200);
        let mut m = serde_json::Map::new();
        m.insert(format!("{}_preview", field), serde_json::json!(preview));
        m.insert(format!("{}_bytes", field), serde_json::json!(bytes));
        m.insert(format!("{}_hash64", field), serde_json::json!(hash64(&s)));
        m.insert(
            format!("{}_artifact_rel_path", field),
            serde_json::json!(stored.and_then(|a| a.artifact_rel_path)),
        );
        Ok(serde_json::Value::Object(m))
    }

    fn emit(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        for s in &mut self.sinks {
            s.on_event(ev)?;
        }
        Ok(())
    }

    fn emit_end(&mut self) -> Result<(), Error> {
        for s in &mut self.sinks {
            s.on_end()?;
        }
        Ok(())
    }

    /// 1 ターン実行: messages を元に LLM を呼び、イベントを Sink に流す。
    /// 受信したイベントは即座に Sink へ emit し、ストリーミング表示する。
    /// tool_execution_cap: このターンで実行するツール呼び出しの上限。None なら無制限。
    /// 戻り値: (new_messages, run_state, assistant_text)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn run_once(
        &mut self,
        messages: &[Msg],
        tool_execution_cap: Option<usize>,
    ) -> Result<(Vec<Msg>, RunState, String), Error> {
        self.run_once_impl(messages, tool_execution_cap, true)
    }

    fn run_once_impl(
        &mut self,
        messages: &[Msg],
        tool_execution_cap: Option<usize>,
        tools_enabled: bool,
    ) -> Result<(Vec<Msg>, RunState, String), Error> {
        let (system_opt, query, history) = msgs_to_provider(messages);
        let system_instruction = system_opt.as_deref();
        let messages_count = history.len() + if query.is_empty() { 0 } else { 1 };
        if let Some(ref hub) = self.event_hub {
            hub.emit(Event {
                v: 1,
                session_id: self.session_id.clone(),
                run_id: self.run_id.clone(),
                kind: "provider.requested".to_string(),
                payload: serde_json::json!({ "messages_count": messages_count }),
            });
        }
        let provider_start = std::time::Instant::now();
        let tool_defs = self.tool_registry.list_definitions();
        let tools_ref = if !tools_enabled || tool_defs.is_empty() {
            None
        } else {
            Some(tool_defs.as_slice())
        };
        let collected: Rc<RefCell<Vec<LlmEvent>>> = Rc::new(RefCell::new(Vec::new()));
        let collected_inner = collected.clone();
        let sinks = &mut self.sinks;
        let interrupt_checker = self.interrupt_checker.clone();
        let mut cb = |ev: LlmEvent| -> Result<(), Error> {
            if interrupt_checker
                .as_ref()
                .map_or(false, |c| c.is_interrupted())
            {
                return Err(Error::System(
                    "Interrupted by user (Ctrl+C). State saved for resume.".to_string(),
                ));
            }
            for s in sinks.iter_mut() {
                s.on_event(&AgentEvent::Llm(ev.clone()))?;
            }
            collected_inner.borrow_mut().push(ev);
            Ok(())
        };

        if interrupt_checker
            .as_ref()
            .map_or(false, |c| c.is_interrupted())
        {
            return Err(Error::System(
                "Interrupted by user (Ctrl+C). State saved for resume.".to_string(),
            ));
        }

        self.stream.as_ref().stream_events(
            &query,
            system_instruction,
            &history,
            tools_ref,
            &mut cb,
        )?;
        let latency_ms = provider_start.elapsed().as_millis() as u64;
        if let Some(ref hub) = self.event_hub {
            let finish_reason = collected.borrow().iter().rev().find_map(|ev| match ev {
                LlmEvent::Completed { finish } => Some(finish.clone()),
                _ => None,
            });
            hub.emit(Event {
                v: 1,
                session_id: self.session_id.clone(),
                run_id: self.run_id.clone(),
                kind: "provider.completed".to_string(),
                payload: serde_json::json!({
                    "finish_reason": format!("{:?}", finish_reason.unwrap_or(FinishReason::Other("unknown".into()))),
                    "latency_ms": latency_ms,
                }),
            });
        }

        let mut assistant_text = String::new();
        let mut accumulator = ToolCallAccumulator::new();
        let mut pending_tool_calls: Vec<(String, String, Value, Option<String>)> = Vec::new();
        let mut run_state = RunState::StreamingModel;

        for ev in collected.borrow().iter() {
            match ev {
                LlmEvent::TextDelta(s) | LlmEvent::ReasoningDelta(s) => assistant_text.push_str(s),
                LlmEvent::ToolCallBegin {
                    call_id,
                    name,
                    thought_signature,
                } => {
                    if tools_enabled {
                        accumulator.on_begin(
                            call_id.clone(),
                            name.clone(),
                            thought_signature.clone(),
                        );
                    }
                }
                LlmEvent::ToolCallArgsDelta { json_fragment, .. } => {
                    if tools_enabled {
                        accumulator.on_args_delta(json_fragment.clone());
                    }
                }
                LlmEvent::ToolCallEnd { call_id } => {
                    if tools_enabled {
                        if let Some(tc) = accumulator.on_end(call_id.clone())? {
                            pending_tool_calls.push(tc);
                        }
                        run_state = RunState::ExecutingTools;
                    }
                }
                LlmEvent::Completed { .. } => {
                    if run_state != RunState::ExecutingTools {
                        run_state = RunState::Done;
                    }
                }
                LlmEvent::Failed { message } => return Err(Error::http(message.clone())),
            }
        }

        let mut new_messages = messages.to_vec();
        // ツール呼び出しがあった場合は、その前のテキストも含めて一つの Assistant ターンとして扱う
        if !assistant_text.is_empty() || !pending_tool_calls.is_empty() {
            new_messages.push(Msg::assistant(assistant_text.clone()));
        }

        if run_state == RunState::ExecutingTools && !pending_tool_calls.is_empty() {
            let cap = tool_execution_cap.unwrap_or(usize::MAX);
            for (i, (call_id, name, args, thought_signature)) in
                pending_tool_calls.into_iter().enumerate()
            {
                if i >= cap {
                    break;
                }
                // 履歴にツール呼び出し自体を記録（直前の assistant メッセージに紐付く）
                new_messages.push(Msg::tool_call(
                    call_id.clone(),
                    name.clone(),
                    args.clone(),
                    thought_signature.clone(),
                ));

                let verdict = self.policy_engine.evaluate_tool_call(
                    name.as_str(),
                    &args,
                    &self.tool_context,
                    self.non_interactive,
                )?;
                let effective_ctx = match verdict {
                    PolicyVerdict::Allow { value, decision } => {
                        if let Some(ref hub) = self.event_hub {
                            let pl = decision.to_event_payload();
                            hub.emit(Event {
                                v: 1,
                                session_id: self.session_id.clone(),
                                run_id: self.run_id.clone(),
                                kind: "policy.evaluated".to_string(),
                                payload: pl.clone(),
                            });
                            self.append_policy_evaluated(pl)?;
                        }
                        value
                    }
                    PolicyVerdict::RequireApproval {
                        value,
                        decision,
                        prompt,
                    } => {
                        if self.non_interactive {
                            let mut deny_decision = decision.clone();
                            deny_decision.status = "blocked".to_string();
                            deny_decision.reason = "approval_required_non_interactive".to_string();
                            if let Some(ref hub) = self.event_hub {
                                let pl = deny_decision.to_event_payload();
                                hub.emit(Event {
                                    v: 1,
                                    session_id: self.session_id.clone(),
                                    run_id: self.run_id.clone(),
                                    kind: "policy.evaluated".to_string(),
                                    payload: pl.clone(),
                                });
                                self.append_policy_evaluated(pl)?;
                            }
                            let msg = "PermissionDenied: non-interactive mode".to_string();
                            self.emit(&AgentEvent::ToolError {
                                call_id: call_id.clone(),
                                name: name.clone(),
                                args: args.clone(),
                                message: msg.clone(),
                            })?;
                            new_messages.push(Msg::tool_result(
                                &call_id,
                                &name,
                                serde_json::json!({ "error": msg }),
                            ));
                            continue;
                        }
                        match self.approver.approve_unsafe_shell(&prompt) {
                            Ok(Approval::Approved) => {
                                let mut approved_decision = decision;
                                approved_decision.reason = "user_approved".to_string();
                                if let Some(ref hub) = self.event_hub {
                                    let pl = approved_decision.to_event_payload();
                                    hub.emit(Event {
                                        v: 1,
                                        session_id: self.session_id.clone(),
                                        run_id: self.run_id.clone(),
                                        kind: "policy.evaluated".to_string(),
                                        payload: pl.clone(),
                                    });
                                    self.append_policy_evaluated(pl)?;
                                }
                                value
                            }
                            Ok(Approval::Denied) => {
                                let mut denied_decision = decision;
                                denied_decision.status = "blocked".to_string();
                                denied_decision.reason = "denied_by_user".to_string();
                                if let Some(ref hub) = self.event_hub {
                                    let pl = denied_decision.to_event_payload();
                                    hub.emit(Event {
                                        v: 1,
                                        session_id: self.session_id.clone(),
                                        run_id: self.run_id.clone(),
                                        kind: "policy.evaluated".to_string(),
                                        payload: pl.clone(),
                                    });
                                    self.append_policy_evaluated(pl)?;
                                }
                                let msg = "denied by user".to_string();
                                self.emit(&AgentEvent::ToolError {
                                    call_id: call_id.clone(),
                                    name: name.clone(),
                                    args: args.clone(),
                                    message: msg.clone(),
                                })?;
                                new_messages.push(Msg::tool_result(
                                    &call_id,
                                    &name,
                                    serde_json::json!({ "error": msg }),
                                ));
                                continue;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    PolicyVerdict::Deny { decision } => {
                        if let Some(ref hub) = self.event_hub {
                            let pl = decision.to_event_payload();
                            hub.emit(Event {
                                v: 1,
                                session_id: self.session_id.clone(),
                                run_id: self.run_id.clone(),
                                kind: "policy.evaluated".to_string(),
                                payload: pl.clone(),
                            });
                            self.append_policy_evaluated(pl)?;
                        }
                        let msg = "PermissionDenied: blocked by policy".to_string();
                        self.emit(&AgentEvent::ToolError {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            args: args.clone(),
                            message: msg.clone(),
                        })?;
                        new_messages.push(Msg::tool_result(
                            &call_id,
                            &name,
                            serde_json::json!({ "error": msg }),
                        ));
                        continue;
                    }
                };

                // events.ndjson: tool call started（巨大 args は artifacts 参照）
                {
                    let mut payload = serde_json::Map::new();
                    payload.insert("call_id".to_string(), serde_json::json!(call_id));
                    payload.insert("tool_id".to_string(), serde_json::json!(name));
                    let args_pl = self.tool_payload_with_artifact(
                        "args",
                        "tool_call_args",
                        &format!("{}#{}", name, call_id),
                        "application/json",
                        &args,
                    )?;
                    if let Some(obj) = args_pl.as_object() {
                        for (k, v) in obj {
                            payload.insert(k.clone(), v.clone());
                        }
                    }
                    self.append_event("tool.call.started", serde_json::Value::Object(payload))?;
                }

                let exec_start = std::time::Instant::now();
                match self
                    .tool_registry
                    .call(name.as_str(), args.clone(), &effective_ctx)
                {
                    Ok(result) => {
                        let elapsed_ms = exec_start.elapsed().as_millis() as u64;
                        // events.ndjson: tool call completed（巨大 result は artifacts 参照）
                        let mut payload = serde_json::Map::new();
                        payload.insert("call_id".to_string(), serde_json::json!(call_id));
                        payload.insert("tool_id".to_string(), serde_json::json!(name));
                        payload.insert("elapsed_ms".to_string(), serde_json::json!(elapsed_ms));
                        let res_pl = self.tool_payload_with_artifact(
                            "result",
                            "tool_call_result",
                            &format!("{}#{}", name, call_id),
                            "application/json",
                            &result,
                        )?;
                        if let Some(obj) = res_pl.as_object() {
                            for (k, v) in obj {
                                payload.insert(k.clone(), v.clone());
                            }
                        }
                        self.append_event(
                            "tool.call.completed",
                            serde_json::Value::Object(payload),
                        )?;

                        self.emit(&AgentEvent::ToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            args: args.clone(),
                            result: result.clone(),
                        })?;
                        new_messages.push(Msg::tool_result(&call_id, &name, result));
                    }
                    Err(e) => {
                        let elapsed_ms = exec_start.elapsed().as_millis() as u64;
                        let msg = e.to_string();
                        // events.ndjson: tool call failed（巨大 message は preview のみ）
                        let mut payload = serde_json::Map::new();
                        payload.insert("call_id".to_string(), serde_json::json!(call_id));
                        payload.insert("tool_id".to_string(), serde_json::json!(name));
                        payload.insert("elapsed_ms".to_string(), serde_json::json!(elapsed_ms));
                        payload.insert(
                            "error".to_string(),
                            serde_json::json!(Self::preview_str(&msg, 200)),
                        );
                        self.append_event("tool.call.failed", serde_json::Value::Object(payload))?;

                        self.emit(&AgentEvent::ToolError {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            args: args.clone(),
                            message: msg.clone(),
                        })?;
                        new_messages.push(Msg::tool_result(
                            &call_id,
                            &name,
                            serde_json::json!({ "error": msg }),
                        ));
                    }
                }
            }
            // ツール実行後は ExecutingTools のまま返し、run_until_done が再度 LLM を呼べるようにする
        }

        if run_state == RunState::Done {
            self.emit_end()?;
        }

        Ok((new_messages, run_state, assistant_text))
    }

    /// ツール実行後に再度 LLM を呼ぶループ。Done になるか max_turns / ツール数上限に達するまで run_once を繰り返す。
    /// 上限到達時は `QueryLoopOutcome::ReachedLimit` を返し、呼び出し元で「続けますか？」等の判断ができる。
    /// - max_turns: LLM 往復回数の上限（1回の応答に複数ツール呼び出しが含まれる場合でも1ターンと数える）
    /// - max_additional_tool_calls: この run で「あと何件まで」ツール実行してよいか（既存件数に加算する）。続行時も同じ値を渡すと、その分だけ追加で実行できる。
    pub fn run_until_done(
        &mut self,
        initial_messages: &[Msg],
        max_turns: usize,
        max_additional_tool_calls: usize,
    ) -> Result<QueryLoopOutcome, Error> {
        let initial_tool_count = count_tool_results(initial_messages);
        let max_tool_calls = initial_tool_count.saturating_add(max_additional_tool_calls);
        let mut messages = initial_messages.to_vec();
        let mut last_assistant_text = String::new();
        let mut last_state = RunState::StreamingModel;

        for _ in 0..max_turns {
            let current_tool_count = count_tool_results(&messages);
            if current_tool_count >= max_tool_calls {
                // messages 末尾が ToolResult かつ text が空なら finalization を試す
                if messages
                    .last()
                    .map_or(false, |m| matches!(m, Msg::ToolResult { .. }))
                    && last_assistant_text.trim().is_empty()
                {
                    let (msgs2, _state2, text2) = self.run_once_impl(&messages, Some(0), false)?;
                    messages = msgs2;
                    last_assistant_text = if text2.trim().is_empty() {
                        "Query loop reached the limit after tool execution. State saved for resume. Run `ai --continue` or increase AI_MAX_TURNS / AI_MAX_TOOL_CALLS.".to_string()
                    } else {
                        text2
                    };
                }
                return Ok(QueryLoopOutcome::ReachedLimit(
                    messages,
                    last_assistant_text,
                ));
            }
            let cap = max_tool_calls.saturating_sub(current_tool_count);
            let (new_messages, state, assistant_text) =
                self.run_once_impl(&messages, Some(cap), true)?;
            last_assistant_text = assistant_text;
            last_state = state.clone();
            let tool_count_after = count_tool_results(&new_messages);
            messages = new_messages;

            if tool_count_after >= max_tool_calls {
                // messages 末尾が ToolResult かつ text が空なら finalization を試す
                if messages
                    .last()
                    .map_or(false, |m| matches!(m, Msg::ToolResult { .. }))
                    && last_assistant_text.trim().is_empty()
                {
                    let (msgs2, _state2, text2) = self.run_once_impl(&messages, Some(0), false)?;
                    messages = msgs2;
                    last_assistant_text = if text2.trim().is_empty() {
                        "Query loop reached the limit after tool execution. State saved for resume. Run `ai --continue` or increase AI_MAX_TURNS / AI_MAX_TOOL_CALLS.".to_string()
                    } else {
                        text2
                    };
                }
                return Ok(QueryLoopOutcome::ReachedLimit(
                    messages,
                    last_assistant_text,
                ));
            }

            match state {
                RunState::Done => return Ok(QueryLoopOutcome::Done(messages, last_assistant_text)),
                RunState::ExecutingTools => continue,
                RunState::StreamingModel | RunState::Error => {
                    return Ok(QueryLoopOutcome::Done(messages, last_assistant_text));
                }
            }
        }

        // max_turns 到達時
        if last_state == RunState::ExecutingTools {
            let (msgs2, _state2, text2) = self.run_once_impl(&messages, Some(0), false)?;
            messages = msgs2;
            last_assistant_text = if text2.trim().is_empty() {
                "Query loop reached the limit after tool execution. State saved for resume. Run `ai --continue` or increase AI_MAX_TURNS / AI_MAX_TOOL_CALLS.".to_string()
            } else {
                text2
            };
        }

        Ok(QueryLoopOutcome::ReachedLimit(
            messages,
            last_assistant_text,
        ))
    }
}

impl QueryLoopRunner for QueryLoop {
    fn run_until_done(
        &mut self,
        initial_messages: &[Msg],
        max_turns: usize,
        max_additional_tool_calls: usize,
    ) -> Result<QueryLoopOutcome, Error> {
        QueryLoop::run_until_done(self, initial_messages, max_turns, max_additional_tool_calls)
    }
}
