//! 責務: history.reduce の結果を監査用の HistoryPackDecision に変換するのみ。I/O を知らない。

use crate::domain::BudgetDecision;
use common::llm::provider::Message as LlmMessage;

/// 履歴リデュースの監査結果（決定内容）を表す。
#[derive(Debug, Clone)]
pub struct HistoryPackDecision {
    pub reduced_messages: Vec<LlmMessage>,
    pub decisions: Vec<BudgetDecision>,
    pub input_count: usize,
    pub input_chars: usize,
    pub output_count: usize,
    pub output_chars: usize,
}

/// history.reduce の結果から監査用の HistoryPackDecision を生成する純関数。
///
/// - `all_messages`: reducer に渡した元の履歴（＋オプションのクエリ）
/// - `reduced_messages`: reducer から返された履歴
/// - `char_counter`: 1 メッセージ当たりの概算文字数を返す関数
/// - `internal_msgs_count`: （任意）system などを含めた内部メッセージ数を監査用に含めたい場合に指定する
pub fn decide_history_pack<F>(
    all_messages: &[LlmMessage],
    reduced_messages: Vec<LlmMessage>,
    char_counter: F,
    internal_msgs_count: Option<usize>,
) -> HistoryPackDecision
where
    F: Fn(&LlmMessage) -> usize,
{
    let input_count = all_messages.len();
    let output_count = reduced_messages.len();

    let input_chars = all_messages.iter().map(&char_counter).sum();
    let output_chars = reduced_messages.iter().map(&char_counter).sum();

    let action = if output_count == input_count {
        "keep"
    } else {
        "truncate"
    };

    let mut details = serde_json::json!({
        "input_messages": input_count,
        "output_messages": output_count,
        "input_chars": input_chars,
        "output_chars": output_chars,
    });

    if let Some(internal) = internal_msgs_count {
        if let Some(obj) = details.as_object_mut() {
            obj.insert(
                "internal_msgs_count".to_string(),
                serde_json::json!(internal),
            );
        }
    }

    let decisions = vec![BudgetDecision {
        stage: "history.reduce".to_string(),
        action: action.to_string(),
        reason: "history_reducer".to_string(),
        details,
    }];

    HistoryPackDecision {
        reduced_messages,
        decisions,
        input_count,
        input_chars,
        output_count,
        output_chars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> LlmMessage {
        LlmMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        }
    }

    #[test]
    fn keep_when_input_and_output_lengths_match() {
        let all = vec![msg("user", "hello"), msg("assistant", "world")];
        let reduced = all.clone();

        let decision = decide_history_pack(&all, reduced, |m| m.content.len(), None);

        assert_eq!(decision.input_count, 2);
        assert_eq!(decision.output_count, 2);
        assert_eq!(decision.input_chars, "hello".len() + "world".len());
        assert_eq!(decision.output_chars, "hello".len() + "world".len());

        assert_eq!(decision.decisions.len(), 1);
        let d = &decision.decisions[0];
        assert_eq!(d.stage, "history.reduce");
        assert_eq!(d.action, "keep");
        assert_eq!(d.reason, "history_reducer");
        assert_eq!(d.details["input_messages"], 2);
        assert_eq!(d.details["output_messages"], 2);
    }

    #[test]
    fn truncate_when_output_is_shorter() {
        let all = vec![
            msg("user", "one"),
            msg("assistant", "two"),
            msg("user", "three"),
        ];
        let reduced = vec![msg("user", "one"), msg("assistant", "two")];

        let decision = decide_history_pack(&all, reduced, |m| m.content.len(), None);

        assert_eq!(decision.input_count, 3);
        assert_eq!(decision.output_count, 2);
        assert_eq!(decision.decisions.len(), 1);
        assert_eq!(decision.decisions[0].action, "truncate");
    }

    #[test]
    fn internal_msgs_count_is_included_when_provided() {
        let all = vec![msg("user", "one")];
        let reduced = vec![msg("user", "one")];

        let decision = decide_history_pack(&all, reduced, |m| m.content.len(), Some(3));

        let d = &decision.decisions[0];
        assert_eq!(d.details["internal_msgs_count"], 3);
    }

    #[test]
    fn internal_msgs_count_is_omitted_when_none() {
        let all = vec![msg("user", "one")];
        let reduced = vec![msg("user", "one")];

        let decision = decide_history_pack(&all, reduced, |m| m.content.len(), None);

        let d = &decision.decisions[0];
        assert!(d.details.get("internal_msgs_count").is_none());
    }
}
