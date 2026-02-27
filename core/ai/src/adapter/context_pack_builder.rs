//! 履歴＋クエリから ContextPack を構築する標準アダプタ

use crate::domain::{
    Budget, BudgetDecision, BudgetReport, BudgetStats, ContextBudget, ContextPack, HistoryReducer,
    Query,
};
use crate::ports::outbound::{ContextPackBuilder, QueryPlacement};
use common::error::Error;
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
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

fn count_chars(messages: &[LlmMessage]) -> usize {
    messages.iter().map(|m| m.content.len()).sum()
}

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

        let input_count = all_messages.len();
        let input_chars = count_chars(&all_messages);

        let reduced = self.reducer.reduce(&all_messages, self.budget);

        let output_count = reduced.len();
        let output_chars = count_chars(&reduced);

        let mut msgs = Vec::new();
        if let Some(s) = system_instruction {
            msgs.push(Msg::system(s));
        }
        msgs.extend(history_to_msgs(&reduced));

        let action = if output_count == input_count {
            "keep"
        } else {
            "truncate"
        };

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
                message_count: output_count,
                char_count: output_chars,
            },
            decisions: vec![BudgetDecision {
                stage: "history.reduce".to_string(),
                action: action.to_string(),
                reason: "history_reducer".to_string(),
                details: serde_json::json!({
                    "input_messages": input_count,
                    "output_messages": output_count,
                    "input_chars": input_chars,
                    "output_chars": output_chars,
                }),
            }],
        };

        Ok(ContextPack {
            v: 1,
            messages: msgs,
            attachments: vec![],
            budget_report,
        })
    }
}
