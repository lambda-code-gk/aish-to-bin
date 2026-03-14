//! 履歴＋クエリ（＋addons）から ContextPack を構築する標準アダプタ
//!
//! v0.1: StdContextPackBuilder(reducer, budget) は messages + budget_report のみ（attachments 空）。
//! StdContextPackBuilderWithAddons は addons/selectors 対応（既存テスト用）。
//! StdContextPackBuilder 等はテストで使用。
#![allow(dead_code)]

use crate::domain::{
    assemble_context_with_addons, build_budget_report, screen_addons, select_addons_within_budget,
    AddonScreeningDecision, BudgetDecision, ContextAddon, ContextAssemblyPlan, ContextBudget,
    ContextPack, HistoryPackDecision, HistoryReducer, Query, ScreenAddonInput,
    SensitiveCheckResult,
};
use crate::ports::outbound::{
    ContextAddonInput, ContextAddonSelector, ContextPackBuilder, QueryPlacement,
    SensitiveTextFilter,
};
use common::domain::SessionDir;
use common::error::Error;
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
use std::path::PathBuf;
use std::sync::Arc;

fn history_to_msgs(messages: &[LlmMessage]) -> Vec<Msg> {
    let mut msgs = Vec::with_capacity(messages.len());
    for m in messages {
        if m.role == "user" {
            msgs.push(Msg::user(&m.content));
        } else if m.role == "tool" {
            if let Some(ref call_id) = m.tool_call_id {
                let name = m.tool_name.as_deref().unwrap_or("");
                msgs.push(Msg::tool_result(
                    call_id,
                    name,
                    serde_json::from_str(&m.content).unwrap_or(serde_json::json!({})),
                ));
            }
        } else {
            msgs.push(Msg::assistant(&m.content));
            if let Some(ref tool_calls) = m.tool_calls {
                for tc in tool_calls {
                    msgs.push(Msg::tool_call(
                        &tc.id,
                        &tc.name,
                        tc.args.clone(),
                        tc.thought_signature.clone(),
                    ));
                }
            }
        }
    }
    msgs
}

/// 1 件の LlmMessage の概算文字数（content + tool_calls の name/args + tool_call_id + tool_name）。
fn count_chars_llm_message(m: &LlmMessage) -> usize {
    let mut n = m.content.len();
    if let Some(ref tcs) = m.tool_calls {
        for tc in tcs {
            n += tc.name.len();
            n += serde_json::to_string(&tc.args)
                .map(|s| s.len())
                .unwrap_or(0);
        }
    }
    if let Some(ref s) = m.tool_call_id {
        n += s.len();
    }
    if let Some(ref s) = m.tool_name {
        n += s.len();
    }
    n
}

fn count_chars_llm_messages(messages: &[LlmMessage]) -> usize {
    messages.iter().map(count_chars_llm_message).sum()
}

/// v0.1: 履歴＋クエリのみ。attachments は常に空。StdContextMessageBuilder と同等の messages。
pub struct StdContextPackBuilder {
    reducer: Arc<dyn HistoryReducer>,
    budget: ContextBudget,
}

impl StdContextPackBuilder {
    pub fn new(reducer: Arc<dyn HistoryReducer>, budget: ContextBudget) -> Self {
        Self { reducer, budget }
    }
}

impl ContextPackBuilder for StdContextPackBuilder {
    fn build(
        &self,
        history: &[LlmMessage],
        _session_dir: Option<&SessionDir>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        query_placement: QueryPlacement,
    ) -> Result<ContextPack, Error> {
        let all_messages: Vec<LlmMessage> = if query_placement == QueryPlacement::AppendAtEnd {
            if let Some(q) = query {
                let mut v = history.to_vec();
                v.push(LlmMessage::user(q.as_ref()));
                v
            } else {
                history.to_vec()
            }
        } else {
            history.to_vec()
        };

        let mut msgs = Vec::new();
        if let Some(s) = system_instruction {
            msgs.push(Msg::system(s));
        }
        let reduced = self.reducer.reduce(&all_messages, self.budget);
        let internal_msgs_count = msgs.len() + reduced.len();
        let HistoryPackDecision {
            reduced_messages,
            decisions,
            input_count,
            input_chars,
            output_count,
            output_chars,
        } = crate::domain::decide_history_pack(
            &all_messages,
            reduced,
            count_chars_llm_message,
            Some(internal_msgs_count),
        );

        msgs.extend(history_to_msgs(&reduced_messages));
        let budget_report = build_budget_report(
            self.budget,
            input_count,
            input_chars,
            output_count,
            output_chars,
            decisions,
        );

        Ok(ContextPack {
            v: 1,
            messages: msgs,
            attachments: vec![],
            budget_report,
        })
    }
}

/// Addons/selectors 対応の ContextPackBuilder（テスト・将来拡張用）
pub struct StdContextPackBuilderWithAddons {
    reducer: Arc<dyn HistoryReducer>,
    budget: ContextBudget,
    selectors: Vec<Arc<dyn ContextAddonSelector>>,
    addons_budget: ContextBudget,
    project_root: PathBuf,
    sensitive_filter: Option<Arc<dyn SensitiveTextFilter>>,
}

impl StdContextPackBuilderWithAddons {
    pub fn new(
        reducer: Arc<dyn HistoryReducer>,
        budget: ContextBudget,
        selectors: Vec<Arc<dyn ContextAddonSelector>>,
        addons_budget: ContextBudget,
        project_root: PathBuf,
        sensitive_filter: Option<Arc<dyn SensitiveTextFilter>>,
    ) -> Self {
        Self {
            reducer,
            budget,
            selectors,
            addons_budget,
            project_root,
            sensitive_filter,
        }
    }
}

fn msg_text_content(msg: &Msg) -> Option<&str> {
    match msg {
        Msg::System(s) | Msg::User(s) | Msg::Assistant(s) => Some(s.as_str()),
        _ => None,
    }
}

impl ContextPackBuilder for StdContextPackBuilderWithAddons {
    fn build(
        &self,
        history: &[LlmMessage],
        session_dir: Option<&SessionDir>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        query_placement: QueryPlacement,
    ) -> Result<ContextPack, Error> {
        let mut decisions = Vec::new();

        // --- Phase 1: baseline messages (history + query) ---
        let all_messages: Vec<LlmMessage> = if query_placement == QueryPlacement::AppendAtEnd {
            if let Some(q) = query {
                let mut v = history.to_vec();
                v.push(LlmMessage::user(q.as_ref()));
                v
            } else {
                history.to_vec()
            }
        } else {
            history.to_vec()
        };

        let history_budget = ContextBudget {
            max_messages: self
                .budget
                .max_messages
                .saturating_sub(self.addons_budget.max_messages),
            max_chars: self
                .budget
                .max_chars
                .saturating_sub(self.addons_budget.max_chars),
        };
        let reduced = self.reducer.reduce(&all_messages, history_budget);

        let mut msgs = Vec::new();
        if let Some(s) = system_instruction {
            msgs.push(Msg::system(s));
        }
        let HistoryPackDecision {
            reduced_messages,
            decisions: history_decisions,
            input_count,
            input_chars,
            output_count: _,
            output_chars: _,
        } = crate::domain::decide_history_pack(
            &all_messages,
            reduced,
            count_chars_llm_message,
            None,
        );

        decisions.extend(history_decisions);

        msgs.extend(history_to_msgs(&reduced_messages));

        // --- Phase 2: collect addons from selectors ---
        let addon_input = ContextAddonInput {
            history,
            query,
            project_root: &self.project_root,
            session_dir,
        };

        let mut candidates: Vec<ContextAddon> = Vec::new();
        for selector in &self.selectors {
            match selector.select(&addon_input) {
                Ok(addons) => candidates.extend(addons),
                Err(e) => {
                    decisions.push(BudgetDecision {
                        stage: "addon.selector".to_string(),
                        action: "error".to_string(),
                        reason: selector.name().to_string(),
                        details: serde_json::json!({ "error": e.to_string() }),
                    });
                }
            }
        }

        // sort by priority desc (higher = more important)
        candidates.sort_by(|a, b| b.priority.cmp(&a.priority));

        // --- Phase 2.5: sensitive filter on addon candidates (domain で集約) ---
        if let Some(ref filter) = self.sensitive_filter {
            let inputs: Vec<ScreenAddonInput> = candidates
                .into_iter()
                .map(|addon| {
                    let msg_result = match msg_text_content(&addon.msg) {
                        Some(text) => match filter.filter(text) {
                            Ok(outcome) => SensitiveCheckResult::Ok(outcome),
                            Err(e) => SensitiveCheckResult::Err(e.to_string()),
                        },
                        None => SensitiveCheckResult::NotChecked,
                    };
                    let att_result =
                        match addon.attachment.as_ref().and_then(|a| a.content.as_ref()) {
                            Some(content) => match filter.filter(content) {
                                Ok(outcome) => SensitiveCheckResult::Ok(outcome),
                                Err(e) => SensitiveCheckResult::Err(e.to_string()),
                            },
                            None => SensitiveCheckResult::NotChecked,
                        };
                    ScreenAddonInput {
                        addon,
                        msg_outcome: msg_result,
                        attachment_outcome: att_result,
                    }
                })
                .collect();
            let AddonScreeningDecision {
                accepted,
                decisions: addon_decisions,
            } = screen_addons(inputs, 2000);
            decisions.extend(addon_decisions);
            candidates = accepted;
        }

        // --- Phase 3: budget-fit addons (domain pure function) ---
        let baseline_chars = msgs
            .iter()
            .map(|msg| match msg {
                Msg::System(s) | Msg::User(s) | Msg::Assistant(s) => s.len(),
                Msg::ToolCall { args, .. } => {
                    serde_json::to_string(args).map(|s| s.len()).unwrap_or(0)
                }
                Msg::ToolResult { result, .. } => {
                    serde_json::to_string(result).map(|s| s.len()).unwrap_or(0)
                }
            })
            .sum();
        let baseline_count = msgs.len();

        let alloc = select_addons_within_budget(
            candidates,
            self.budget,
            self.addons_budget,
            baseline_count,
            baseline_chars,
        );
        decisions.extend(alloc.decisions);

        // --- Phase 4: assemble final context (messages + attachments + decisions) ---
        let ContextAssemblyPlan {
            messages,
            attachments,
            decisions,
        } = assemble_context_with_addons(msgs, alloc.accepted, query.is_some(), decisions);

        let final_count = messages.len();
        let final_chars: usize = messages
            .iter()
            .map(|msg| match msg {
                Msg::System(s) | Msg::User(s) | Msg::Assistant(s) => s.len(),
                Msg::ToolCall { args, .. } => {
                    serde_json::to_string(args).map(|s| s.len()).unwrap_or(0)
                }
                Msg::ToolResult { result, .. } => {
                    serde_json::to_string(result).map(|s| s.len()).unwrap_or(0)
                }
            })
            .sum();

        let budget_report = build_budget_report(
            self.budget,
            input_count,
            input_chars,
            final_count,
            final_chars,
            decisions,
        );

        Ok(ContextPack {
            v: 1,
            messages,
            attachments,
            budget_report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::llm::provider::Message as LlmMessage;

    #[test]
    fn count_chars_llm_message_content_only() {
        let m = LlmMessage::user("hello");
        assert_eq!(count_chars_llm_message(&m), 5);
    }

    #[test]
    fn count_chars_llm_message_includes_tool_calls_and_metadata() {
        let m = LlmMessage::assistant_with_tool_calls(
            "think",
            vec![(
                "call_1".to_string(),
                "run_shell".to_string(),
                serde_json::json!({"x": 1}),
                None,
            )],
        );
        let n = count_chars_llm_message(&m);
        assert!(n >= "think".len());
        assert!(n > 5, "should count tool name and args");
    }

    #[test]
    fn count_chars_llm_messages_sums_all() {
        let messages = vec![LlmMessage::user("ab"), LlmMessage::user("c")];
        assert_eq!(count_chars_llm_messages(&messages), 3);
    }
}
