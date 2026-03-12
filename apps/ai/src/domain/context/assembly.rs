//! 責務: baseline messages と accepted addons から ContextPack 用の messages/attachments/decisions を組み立てるのみ。I/O を知らない。

use crate::domain::{addon_insertion_index, BudgetDecision, ContextAddon, ContextAttachment};
use common::msg::Msg;

/// accepted addons を適用したあとのコンテキスト組立結果。
#[derive(Debug, Clone)]
pub struct ContextAssemblyPlan {
    pub messages: Vec<Msg>,
    pub attachments: Vec<ContextAttachment>,
    pub decisions: Vec<BudgetDecision>,
}

/// baseline messages に accepted addons を適用して最終的な messages/attachments/decisions を返す。
///
/// - `baseline_messages`: system＋reduced history 等、addons 適用前のメッセージ列
/// - `accepted_addons`: budget 内に収まると判断された addons
/// - `has_query`: クエリが存在するかどうか（挿入位置判定に使用）
/// - `carried_decisions`: それまでに蓄積された BudgetDecision（assembly では新たに追加しない）
pub fn assemble_context_with_addons(
    mut baseline_messages: Vec<Msg>,
    accepted_addons: Vec<ContextAddon>,
    has_query: bool,
    carried_decisions: Vec<BudgetDecision>,
) -> ContextAssemblyPlan {
    let mut attachments = Vec::new();
    for addon in &accepted_addons {
        if let Some(ref att) = addon.attachment {
            attachments.push(att.clone());
        }
    }

    if !accepted_addons.is_empty() {
        let insert_pos = addon_insertion_index(&baseline_messages, has_query);
        let addon_msgs: Vec<Msg> = accepted_addons.into_iter().map(|a| a.msg).collect();
        for (i, m) in addon_msgs.into_iter().enumerate() {
            baseline_messages.insert(insert_pos + i, m);
        }
    }

    ContextAssemblyPlan {
        messages: baseline_messages,
        attachments,
        decisions: carried_decisions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addon(id: &str, msg: Msg, attachment: Option<ContextAttachment>) -> ContextAddon {
        ContextAddon {
            id: id.to_string(),
            kind: "k".to_string(),
            title: "t".to_string(),
            priority: 0,
            msg,
            attachment,
            source: None,
        }
    }

    fn attachment(title: &str) -> ContextAttachment {
        ContextAttachment {
            kind: "file".to_string(),
            title: title.to_string(),
            content_type: "text/plain".to_string(),
            content: Some("body".to_string()),
            artifact_rel_path: None,
            bytes: 4,
            hash64: "h".to_string(),
            source: None,
        }
    }

    #[test]
    fn collects_attachments_from_accepted_addons() {
        let baseline = vec![Msg::system("s")];
        let addons = vec![addon(
            "a1",
            Msg::user("u".to_string()),
            Some(attachment("att1")),
        )];
        let carried = vec![];

        let plan = assemble_context_with_addons(baseline, addons, false, carried);

        assert_eq!(plan.attachments.len(), 1);
        assert_eq!(plan.attachments[0].title, "att1");
    }

    #[test]
    fn inserts_addon_before_last_user_when_query_exists() {
        let baseline = vec![
            Msg::system("s"),
            Msg::assistant("a"),
            Msg::user("query".to_string()),
        ];
        let addons = vec![addon("a1", Msg::user("addon".to_string()), None)];
        let carried = vec![];

        let plan = assemble_context_with_addons(baseline, addons, true, carried);

        // system, assistant, addon, query
        assert_eq!(plan.messages.len(), 4);
        assert!(matches!(plan.messages[2], Msg::User(ref s) if s == "addon"));
        assert!(matches!(plan.messages[3], Msg::User(ref s) if s == "query"));
    }

    #[test]
    fn inserts_addon_at_end_when_no_query() {
        let baseline = vec![Msg::system("s"), Msg::assistant("a")];
        let addons = vec![addon("a1", Msg::user("addon".to_string()), None)];
        let carried = vec![];

        let plan = assemble_context_with_addons(baseline, addons, false, carried);

        assert_eq!(plan.messages.len(), 3);
        assert!(matches!(plan.messages[2], Msg::User(ref s) if s == "addon"));
    }

    #[test]
    fn carries_decisions_unchanged() {
        let baseline = vec![Msg::system("s")];
        let addons = Vec::new();
        let carried = vec![BudgetDecision {
            stage: "test".to_string(),
            action: "keep".to_string(),
            reason: "r".to_string(),
            details: serde_json::json!({}),
        }];

        let plan = assemble_context_with_addons(baseline, addons, false, carried.clone());
        assert_eq!(plan.decisions, carried);
    }
}
