//! 責務: addon 候補の選別（priority ソート・budget-fit 判定）のみ。I/O を知らない。

use super::{BudgetDecision, ContextAddon, ContextBudget};

/// addon 候補の priority 降順ソート結果と、budget 内に収まる候補の選定結果。
#[derive(Debug, Clone)]
pub struct AddonAllocationResult {
    pub accepted: Vec<ContextAddon>,
    pub decisions: Vec<BudgetDecision>,
}

fn msg_char_len(msg: &common::msg::Msg) -> usize {
    use common::msg::Msg;
    match msg {
        Msg::System(s) | Msg::User(s) | Msg::Assistant(s) => s.len(),
        Msg::ToolCall { args, .. } => serde_json::to_string(args).map(|s| s.len()).unwrap_or(0),
        Msg::ToolResult { result, .. } => {
            serde_json::to_string(result).map(|s| s.len()).unwrap_or(0)
        }
    }
}

/// priority 降順にソートし、budget に収まる addon だけを選別する純関数。
pub fn select_addons_within_budget(
    mut candidates: Vec<ContextAddon>,
    overall_budget: ContextBudget,
    addons_budget: ContextBudget,
    baseline_msg_count: usize,
    baseline_char_count: usize,
) -> AddonAllocationResult {
    candidates.sort_by(|a, b| b.priority.cmp(&a.priority));

    let remaining_msgs = overall_budget
        .max_messages
        .saturating_sub(baseline_msg_count);
    let remaining_chars = overall_budget.max_chars.saturating_sub(baseline_char_count);

    let msg_limit = remaining_msgs.min(addons_budget.max_messages);
    let char_limit = remaining_chars.min(addons_budget.max_chars);

    let mut used_msgs = 0usize;
    let mut used_chars = 0usize;
    let mut accepted = Vec::new();
    let mut decisions = Vec::new();

    for addon in candidates {
        let attachment_chars = addon
            .attachment
            .as_ref()
            .and_then(|a| a.content.as_ref())
            .map(|s| s.len())
            .unwrap_or(0);
        let addon_chars = msg_char_len(&addon.msg) + attachment_chars;

        if used_msgs + 1 > msg_limit || used_chars + addon_chars > char_limit {
            decisions.push(BudgetDecision {
                stage: "addon.select".to_string(),
                action: "drop".to_string(),
                reason: "budget".to_string(),
                details: serde_json::json!({
                    "addon_id": addon.id,
                    "addon_chars": addon_chars,
                    "used_msgs": used_msgs,
                    "used_chars": used_chars,
                    "limit_msgs": msg_limit,
                    "limit_chars": char_limit,
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
    }

    AddonAllocationResult {
        accepted,
        decisions,
    }
}

/// addon メッセージを messages 列に挿入する位置を返す。
/// query がある場合は最後の User メッセージの直前、なければ末尾。
pub fn addon_insertion_index(messages: &[common::msg::Msg], has_query: bool) -> usize {
    if !has_query {
        return messages.len();
    }
    messages
        .iter()
        .rposition(|m| matches!(m, common::msg::Msg::User(_)))
        .unwrap_or(messages.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::msg::Msg;

    fn addon(id: &str, priority: u32, char_count: usize) -> ContextAddon {
        let content = "x".repeat(char_count);
        ContextAddon {
            id: id.to_string(),
            kind: "test".to_string(),
            title: id.to_string(),
            priority,
            msg: Msg::user(content),
            attachment: None,
            source: None,
        }
    }

    #[test]
    fn sorts_by_priority_desc_and_respects_budget() {
        let candidates = vec![addon("low", 10, 50), addon("high", 90, 50)];
        let overall = ContextBudget {
            max_messages: 100,
            max_chars: 100_000,
        };
        let addons = ContextBudget {
            max_messages: 1,
            max_chars: 100_000,
        };
        let result = select_addons_within_budget(candidates, overall, addons, 0, 0);
        assert_eq!(result.accepted.len(), 1);
        assert_eq!(result.accepted[0].id, "high");
    }

    #[test]
    fn drops_when_char_budget_exceeded() {
        let candidates = vec![addon("a", 50, 600), addon("b", 40, 600)];
        let overall = ContextBudget {
            max_messages: 100,
            max_chars: 1000,
        };
        let addons = ContextBudget {
            max_messages: 100,
            max_chars: 1000,
        };
        let result = select_addons_within_budget(candidates, overall, addons, 0, 0);
        assert_eq!(result.accepted.len(), 1);
        assert_eq!(result.accepted[0].id, "a");
        assert_eq!(result.decisions.len(), 2);
        assert_eq!(result.decisions[1].action, "drop");
    }

    #[test]
    fn insertion_index_before_last_user() {
        let msgs = vec![
            Msg::system("sys"),
            Msg::assistant("hi"),
            Msg::user("question"),
        ];
        assert_eq!(addon_insertion_index(&msgs, true), 2);
    }

    #[test]
    fn insertion_index_at_end_when_no_query() {
        let msgs = vec![Msg::system("sys"), Msg::assistant("hi")];
        assert_eq!(addon_insertion_index(&msgs, false), 2);
    }
}
