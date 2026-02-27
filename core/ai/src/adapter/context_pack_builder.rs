//! 履歴＋クエリ＋addons から ContextPack を構築する標準アダプタ

use crate::domain::{
    Budget, BudgetDecision, BudgetReport, BudgetStats, ContextAddon, ContextBudget, ContextPack,
    HistoryReducer, Query, SensitiveFilterOutcome,
};
use crate::ports::outbound::{
    ContextAddonInput, ContextAddonSelector, ContextPackBuilder, QueryPlacement,
    SensitiveTextFilter,
};
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

fn count_chars_llm(messages: &[LlmMessage]) -> usize {
    messages.iter().map(|m| m.content.len()).sum()
}

fn msg_char_len(msg: &Msg) -> usize {
    match msg {
        Msg::System(s) | Msg::User(s) | Msg::Assistant(s) => s.len(),
        Msg::ToolCall { args, .. } => serde_json::to_string(args).map(|s| s.len()).unwrap_or(0),
        Msg::ToolResult { result, .. } => serde_json::to_string(result).map(|s| s.len()).unwrap_or(0),
    }
}

fn msgs_char_total(msgs: &[Msg]) -> usize {
    msgs.iter().map(msg_char_len).sum()
}

pub struct StdContextPackBuilder {
    reducer: Arc<dyn HistoryReducer>,
    budget: ContextBudget,
    selectors: Vec<Arc<dyn ContextAddonSelector>>,
    addons_budget: ContextBudget,
    project_root: PathBuf,
    sensitive_filter: Option<Arc<dyn SensitiveTextFilter>>,
}

impl StdContextPackBuilder {
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

fn replace_msg_text(msg: &Msg, new_text: String) -> Msg {
    match msg {
        Msg::System(_) => Msg::system(new_text),
        Msg::User(_) => Msg::user(new_text),
        Msg::Assistant(_) => Msg::assistant(new_text),
        other => other.clone(),
    }
}

fn truncate_verbose(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...(truncated)", &s[..max])
    }
}

impl ContextPackBuilder for StdContextPackBuilder {
    fn build(
        &self,
        history: &[LlmMessage],
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

        let input_count = all_messages.len();
        let input_chars = count_chars_llm(&all_messages);

        let history_budget = ContextBudget {
            max_messages: self.budget.max_messages.saturating_sub(self.addons_budget.max_messages),
            max_chars: self.budget.max_chars.saturating_sub(self.addons_budget.max_chars),
        };
        let reduced = self.reducer.reduce(&all_messages, history_budget);

        let output_count = reduced.len();
        let output_chars = count_chars_llm(&reduced);

        let history_action = if output_count == input_count { "keep" } else { "truncate" };
        decisions.push(BudgetDecision {
            stage: "history.reduce".to_string(),
            action: history_action.to_string(),
            reason: "history_reducer".to_string(),
            details: serde_json::json!({
                "input_messages": input_count,
                "output_messages": output_count,
                "input_chars": input_chars,
                "output_chars": output_chars,
            }),
        });

        let mut msgs = Vec::new();
        if let Some(s) = system_instruction {
            msgs.push(Msg::system(s));
        }
        msgs.extend(history_to_msgs(&reduced));

        // --- Phase 2: collect addons from selectors ---
        let addon_input = ContextAddonInput {
            history,
            query,
            project_root: &self.project_root,
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

        // --- Phase 2.5: sensitive filter on addon candidates ---
        if let Some(ref filter) = self.sensitive_filter {
            let mut filtered = Vec::with_capacity(candidates.len());
            for mut addon in candidates {
                let mut denied = false;
                if let Some(text) = msg_text_content(&addon.msg) {
                    match filter.filter(text) {
                        Ok(SensitiveFilterOutcome::Clean) => {}
                        Ok(SensitiveFilterOutcome::Deny { verbose }) => {
                            decisions.push(BudgetDecision {
                                stage: "addon.sensitive".to_string(),
                                action: "deny".to_string(),
                                reason: "leakscan".to_string(),
                                details: serde_json::json!({
                                    "addon_id": addon.id,
                                    "kind": addon.kind,
                                    "target": "msg",
                                    "verbose": truncate_verbose(&verbose, 2000),
                                }),
                            });
                            denied = true;
                        }
                        Ok(SensitiveFilterOutcome::Masked { masked, verbose }) => {
                            decisions.push(BudgetDecision {
                                stage: "addon.sensitive".to_string(),
                                action: "mask".to_string(),
                                reason: "leakscan".to_string(),
                                details: serde_json::json!({
                                    "addon_id": addon.id,
                                    "kind": addon.kind,
                                    "target": "msg",
                                    "verbose": truncate_verbose(&verbose, 2000),
                                }),
                            });
                            addon.msg = replace_msg_text(&addon.msg, masked);
                        }
                        Err(_) => {}
                    }
                }
                if denied {
                    continue;
                }
                if let Some(ref att) = addon.attachment {
                    if let Some(ref content) = att.content {
                        match filter.filter(content) {
                            Ok(SensitiveFilterOutcome::Clean) => {}
                            Ok(SensitiveFilterOutcome::Deny { verbose }) => {
                                decisions.push(BudgetDecision {
                                    stage: "addon.sensitive".to_string(),
                                    action: "deny".to_string(),
                                    reason: "leakscan".to_string(),
                                    details: serde_json::json!({
                                        "addon_id": addon.id,
                                        "kind": addon.kind,
                                        "target": "attachment",
                                        "verbose": truncate_verbose(&verbose, 2000),
                                    }),
                                });
                                continue;
                            }
                            Ok(SensitiveFilterOutcome::Masked { masked, verbose }) => {
                                decisions.push(BudgetDecision {
                                    stage: "addon.sensitive".to_string(),
                                    action: "mask".to_string(),
                                    reason: "leakscan".to_string(),
                                    details: serde_json::json!({
                                        "addon_id": addon.id,
                                        "kind": addon.kind,
                                        "target": "attachment",
                                        "verbose": truncate_verbose(&verbose, 2000),
                                    }),
                                });
                                let new_hash = crate::domain::hash64(&masked);
                                let new_bytes = masked.len() as u64;
                                let mut new_att = att.clone();
                                new_att.content = Some(masked);
                                new_att.bytes = new_bytes;
                                new_att.hash64 = new_hash;
                                addon.attachment = Some(new_att);
                            }
                            Err(_) => {}
                        }
                    }
                }
                filtered.push(addon);
            }
            candidates = filtered;
        }

        // --- Phase 3: budget-fit addons ---
        let baseline_chars = msgs_char_total(&msgs);
        let baseline_count = msgs.len();

        let remaining_msgs = self.budget.max_messages.saturating_sub(baseline_count);
        let remaining_chars = self.budget.max_chars.saturating_sub(baseline_chars);

        let addons_msg_limit = remaining_msgs.min(self.addons_budget.max_messages);
        let addons_char_limit = remaining_chars.min(self.addons_budget.max_chars);

        let mut used_msgs = 0usize;
        let mut used_chars = 0usize;
        let mut accepted: Vec<&ContextAddon> = Vec::new();
        let mut attachments = Vec::new();

        for addon in &candidates {
            let addon_chars = msg_char_len(&addon.msg);
            if used_msgs + 1 > addons_msg_limit || used_chars + addon_chars > addons_char_limit {
                decisions.push(BudgetDecision {
                    stage: "addon.select".to_string(),
                    action: "drop".to_string(),
                    reason: "budget".to_string(),
                    details: serde_json::json!({
                        "addon_id": addon.id,
                        "addon_chars": addon_chars,
                        "used_msgs": used_msgs,
                        "used_chars": used_chars,
                        "limit_msgs": addons_msg_limit,
                        "limit_chars": addons_char_limit,
                    }),
                });
                continue;
            }
            decisions.push(BudgetDecision {
                stage: "addon.select".to_string(),
                action: "keep".to_string(),
                reason: "budget".to_string(),
                details: serde_json::json!({
                    "addon_id": addon.id,
                    "addon_chars": addon_chars,
                }),
            });
            used_msgs += 1;
            used_chars += addon_chars;
            accepted.push(addon);
            if let Some(ref att) = addon.attachment {
                attachments.push(att.clone());
            }
        }

        // --- Phase 4: insert addon messages before the last user message (query) ---
        if !accepted.is_empty() {
            let addon_msgs: Vec<Msg> = accepted.iter().map(|a| a.msg.clone()).collect();
            if query.is_some() {
                let insert_pos = msgs.iter().rposition(|m| matches!(m, Msg::User(_)));
                match insert_pos {
                    Some(pos) => {
                        for (i, m) in addon_msgs.into_iter().enumerate() {
                            msgs.insert(pos + i, m);
                        }
                    }
                    None => {
                        msgs.extend(addon_msgs);
                    }
                }
            } else {
                msgs.extend(addon_msgs);
            }
        }

        let final_count = msgs.len();
        let final_chars = msgs_char_total(&msgs);

        let budget_report = BudgetReport {
            v: 1,
            budget: Budget {
                max_messages: self.budget.max_messages,
                max_chars: self.budget.max_chars,
            },
            input: BudgetStats {
                message_count: input_count,
                char_count: input_chars,
            },
            output: BudgetStats {
                message_count: final_count,
                char_count: final_chars,
            },
            decisions,
        };

        Ok(ContextPack {
            v: 1,
            messages: msgs,
            attachments,
            budget_report,
        })
    }
}
