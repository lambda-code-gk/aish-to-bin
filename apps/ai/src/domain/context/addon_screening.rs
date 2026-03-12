//! 責務: sensitive filter の結果を ContextAddon の keep/drop/mask と BudgetDecision に変換するのみ。I/O を知らない。

use crate::domain::{hash64, BudgetDecision, ContextAddon, SensitiveFilterOutcome};

/// sensitive filter の結果（エラーも含む）を domain で扱うための中間表現。
#[derive(Debug, Clone, PartialEq)]
pub enum SensitiveCheckResult {
    NotChecked,
    Ok(SensitiveFilterOutcome),
    Err(String),
}

/// screening の結果、受理された addon と生成された BudgetDecision 一覧。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AddonScreeningDecision {
    pub accepted: Vec<ContextAddon>,
    pub decisions: Vec<BudgetDecision>,
}

fn truncate_verbose(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...(truncated)", &s[..end])
    }
}

fn details_for(
    addon: &ContextAddon,
    target: &str,
    verbose: &str,
    max_verbose: usize,
) -> serde_json::Value {
    serde_json::json!({
        "addon_id": addon.id,
        "kind": addon.kind,
        "target": target,
        "verbose": truncate_verbose(verbose, max_verbose),
    })
}

fn apply_msg_result(
    mut addon: ContextAddon,
    msg_result: SensitiveCheckResult,
    max_verbose: usize,
) -> (Option<ContextAddon>, Vec<BudgetDecision>) {
    let mut decisions = Vec::new();

    match msg_result {
        SensitiveCheckResult::NotChecked => {}
        SensitiveCheckResult::Ok(SensitiveFilterOutcome::Clean) => {}
        SensitiveCheckResult::Ok(SensitiveFilterOutcome::Hit { verbose }) => {
            decisions.push(BudgetDecision {
                stage: "addon.sensitive".to_string(),
                action: "allow".to_string(),
                reason: "leakscan".to_string(),
                details: details_for(&addon, "msg", &verbose, max_verbose),
            });
        }
        SensitiveCheckResult::Ok(SensitiveFilterOutcome::Deny { verbose }) => {
            decisions.push(BudgetDecision {
                stage: "addon.sensitive".to_string(),
                action: "deny".to_string(),
                reason: "leakscan".to_string(),
                details: details_for(&addon, "msg", &verbose, max_verbose),
            });
            return (None, decisions);
        }
        SensitiveCheckResult::Ok(SensitiveFilterOutcome::Masked { masked, verbose }) => {
            decisions.push(BudgetDecision {
                stage: "addon.sensitive".to_string(),
                action: "mask".to_string(),
                reason: "leakscan".to_string(),
                details: details_for(&addon, "msg", &verbose, max_verbose),
            });
            // 役割はそのままにテキストだけを差し替える
            addon.msg = match addon.msg {
                common::msg::Msg::System(_) => common::msg::Msg::system(masked),
                common::msg::Msg::User(_) => common::msg::Msg::user(masked),
                common::msg::Msg::Assistant(_) => common::msg::Msg::assistant(masked),
                other => other,
            };
        }
        SensitiveCheckResult::Err(verbose) => {
            decisions.push(BudgetDecision {
                stage: "addon.sensitive".to_string(),
                action: "deny".to_string(),
                reason: "leakscan_error".to_string(),
                details: details_for(&addon, "msg", &verbose, max_verbose),
            });
            return (None, decisions);
        }
    }

    (Some(addon), decisions)
}

fn apply_attachment_result(
    mut addon: ContextAddon,
    att_result: SensitiveCheckResult,
    max_verbose: usize,
) -> (Option<ContextAddon>, Vec<BudgetDecision>) {
    let mut decisions = Vec::new();

    let Some(att) = addon.attachment.clone() else {
        return (Some(addon), decisions);
    };

    match att_result {
        SensitiveCheckResult::NotChecked => {}
        SensitiveCheckResult::Ok(SensitiveFilterOutcome::Clean) => {}
        SensitiveCheckResult::Ok(SensitiveFilterOutcome::Hit { verbose }) => {
            decisions.push(BudgetDecision {
                stage: "addon.sensitive".to_string(),
                action: "allow".to_string(),
                reason: "leakscan".to_string(),
                details: details_for(&addon, "attachment", &verbose, max_verbose),
            });
        }
        SensitiveCheckResult::Ok(SensitiveFilterOutcome::Deny { verbose }) => {
            decisions.push(BudgetDecision {
                stage: "addon.sensitive".to_string(),
                action: "deny".to_string(),
                reason: "leakscan".to_string(),
                details: details_for(&addon, "attachment", &verbose, max_verbose),
            });
            return (None, decisions);
        }
        SensitiveCheckResult::Ok(SensitiveFilterOutcome::Masked { masked, verbose }) => {
            decisions.push(BudgetDecision {
                stage: "addon.sensitive".to_string(),
                action: "mask".to_string(),
                reason: "leakscan".to_string(),
                details: details_for(&addon, "attachment", &verbose, max_verbose),
            });
            let new_hash = hash64(&masked);
            let new_bytes = masked.len() as u64;
            let mut new_att = att;
            new_att.content = Some(masked);
            new_att.bytes = new_bytes;
            new_att.hash64 = new_hash;
            addon.attachment = Some(new_att);
        }
        SensitiveCheckResult::Err(verbose) => {
            decisions.push(BudgetDecision {
                stage: "addon.sensitive".to_string(),
                action: "deny".to_string(),
                reason: "leakscan_error".to_string(),
                details: details_for(&addon, "attachment", &verbose, max_verbose),
            });
            return (None, decisions);
        }
    }

    (Some(addon), decisions)
}

/// 1 件の addon に対して、メッセージと添付の sensitive 結果を適用する。
///
/// 戻り値:
/// - accepted: keep/drop/mask 後に keep する場合は Some(addon)、drop する場合は None
/// - decisions: 生成された BudgetDecision 一覧
pub fn apply_sensitive_outcome_to_addon(
    addon: ContextAddon,
    msg_result: SensitiveCheckResult,
    att_result: SensitiveCheckResult,
    max_verbose: usize,
) -> (Option<ContextAddon>, Vec<BudgetDecision>) {
    let (maybe_after_msg, mut decisions) = apply_msg_result(addon, msg_result, max_verbose);
    let Some(after_msg) = maybe_after_msg else {
        return (None, decisions);
    };

    let (maybe_after_att, att_decisions) =
        apply_attachment_result(after_msg, att_result, max_verbose);
    decisions.extend(att_decisions);
    (maybe_after_att, decisions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ContextAttachment;
    use common::msg::Msg;

    fn base_addon() -> ContextAddon {
        ContextAddon {
            id: "a1".to_string(),
            kind: "test".to_string(),
            title: "t".to_string(),
            priority: 0,
            msg: Msg::user("body".to_string()),
            attachment: None,
            source: None,
        }
    }

    #[test]
    fn clean_keeps_addon_without_decision() {
        let addon = base_addon();
        let (accepted, decisions) = apply_sensitive_outcome_to_addon(
            addon,
            SensitiveCheckResult::Ok(SensitiveFilterOutcome::Clean),
            SensitiveCheckResult::NotChecked,
            2000,
        );
        assert!(accepted.is_some());
        assert!(decisions.is_empty());
    }

    #[test]
    fn hit_adds_allow_decision_but_keeps_addon() {
        let addon = base_addon();
        let (accepted, decisions) = apply_sensitive_outcome_to_addon(
            addon,
            SensitiveCheckResult::Ok(SensitiveFilterOutcome::Hit {
                verbose: "v".to_string(),
            }),
            SensitiveCheckResult::NotChecked,
            2000,
        );
        assert!(accepted.is_some());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, "allow");
        assert_eq!(decisions[0].details["target"], "msg");
    }

    #[test]
    fn deny_drops_addon() {
        let addon = base_addon();
        let (accepted, decisions) = apply_sensitive_outcome_to_addon(
            addon,
            SensitiveCheckResult::Ok(SensitiveFilterOutcome::Deny {
                verbose: "v".to_string(),
            }),
            SensitiveCheckResult::NotChecked,
            2000,
        );
        assert!(accepted.is_none());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, "deny");
    }

    #[test]
    fn masked_replaces_message_text() {
        let addon = base_addon();
        let (accepted, decisions) = apply_sensitive_outcome_to_addon(
            addon,
            SensitiveCheckResult::Ok(SensitiveFilterOutcome::Masked {
                masked: "masked".to_string(),
                verbose: "v".to_string(),
            }),
            SensitiveCheckResult::NotChecked,
            2000,
        );
        let accepted = accepted.expect("should be kept");
        match accepted.msg {
            Msg::User(ref s) => assert_eq!(s, "masked"),
            _ => panic!("expected user msg"),
        }
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, "mask");
    }

    #[test]
    fn attachment_mask_updates_hash_and_bytes() {
        let mut addon = base_addon();
        addon.attachment = Some(ContextAttachment {
            kind: "k".to_string(),
            title: "t".to_string(),
            content_type: "text/plain".to_string(),
            content: Some("secret".to_string()),
            artifact_rel_path: None,
            bytes: 6,
            hash64: hash64("secret"),
            source: None,
        });

        let (accepted, decisions) = apply_sensitive_outcome_to_addon(
            addon,
            SensitiveCheckResult::NotChecked,
            SensitiveCheckResult::Ok(SensitiveFilterOutcome::Masked {
                masked: "masked".to_string(),
                verbose: "v".to_string(),
            }),
            2000,
        );

        let accepted = accepted.expect("should be kept");
        let att = accepted.attachment.expect("attachment");
        assert_eq!(att.content.as_deref(), Some("masked"));
        assert_eq!(att.bytes, "masked".len() as u64);
        assert_eq!(att.hash64, hash64("masked"));
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, "mask");
        assert_eq!(decisions[0].details["target"], "attachment");
    }

    #[test]
    fn error_is_treated_as_deny() {
        let addon = base_addon();
        let (accepted, decisions) = apply_sensitive_outcome_to_addon(
            addon,
            SensitiveCheckResult::Err("boom".to_string()),
            SensitiveCheckResult::NotChecked,
            2000,
        );
        assert!(accepted.is_none());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].action, "deny");
        assert_eq!(decisions[0].reason, "leakscan_error");
    }
}
