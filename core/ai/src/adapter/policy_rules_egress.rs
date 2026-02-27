use crate::adapter::leakscan_text_filter::SensitiveAction;
use crate::domain::{
    hash64, ContextAttachment, ContextPack, PolicyDecision, PolicyVerdict, RuleVerdict,
    SensitiveFilterOutcome,
};
use crate::domain::EgressPolicyRule;
use crate::ports::outbound::SensitiveTextFilter;
use common::error::Error;
use common::msg::Msg;
use std::sync::Arc;

fn truncate_verbose(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...(truncated)", &s[..max])
    }
}

fn msg_text(msg: &Msg) -> Option<&str> {
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

/// reducer の漏れなどに備えた egress 文字数の絶対上限
pub struct EgressBudgetHardCapRule {
    pub max_chars: usize,
}

impl EgressPolicyRule for EgressBudgetHardCapRule {
    fn name(&self) -> &'static str {
        "egress_budget_hard_cap"
    }

    fn evaluate(
        &self,
        pack: &ContextPack,
        _non_interactive: bool,
    ) -> Result<RuleVerdict<ContextPack>, Error> {
        let mut total_chars: usize = 0;
        for m in &pack.messages {
            if let Some(t) = msg_text(m) {
                total_chars = total_chars.saturating_add(t.chars().count());
            }
        }
        for a in &pack.attachments {
            if let Some(ref c) = a.content {
                total_chars = total_chars.saturating_add(c.chars().count());
            }
        }
        if total_chars <= self.max_chars {
            return Ok(RuleVerdict::NoMatch);
        }

        let decision = PolicyDecision {
            v: 1,
            scope: "egress".to_string(),
            subject: "context_pack".to_string(),
            status: "blocked".to_string(),
            reason: "egress_hard_cap".to_string(),
            details: serde_json::json!({
                "max_chars": self.max_chars,
                "actual_chars": total_chars,
            }),
        };
        Ok(RuleVerdict::Verdict(PolicyVerdict::Deny { decision }))
    }
}

pub struct EgressSensitiveRule {
    pub filter: Option<Arc<dyn SensitiveTextFilter>>,
    pub action: SensitiveAction,
}

impl EgressPolicyRule for EgressSensitiveRule {
    fn name(&self) -> &'static str {
        "egress_sensitive"
    }

    fn evaluate(
        &self,
        pack: &ContextPack,
        _non_interactive: bool,
    ) -> Result<RuleVerdict<ContextPack>, Error> {
        let filter = match self.filter {
            Some(ref f) => f,
            None => return Ok(RuleVerdict::NoMatch),
        };

        let mut hit_count = 0usize;
        let mut targets: Vec<String> = Vec::new();
        let mut verbose_all = String::new();
        let mut masked_pack = pack.clone();
        let mut denied = false;

        for (idx, msg) in pack.messages.iter().enumerate() {
            if let Some(text) = msg_text(msg) {
                match filter.filter(text)? {
                    SensitiveFilterOutcome::Clean => {}
                    SensitiveFilterOutcome::Deny { verbose } => {
                        hit_count += 1;
                        targets.push(format!("msg:{}", idx));
                        verbose_all.push_str(&verbose);
                        denied = true;
                    }
                    SensitiveFilterOutcome::Masked { masked, verbose } => {
                        hit_count += 1;
                        targets.push(format!("msg:{}", idx));
                        verbose_all.push_str(&verbose);
                        masked_pack.messages[idx] = replace_msg_text(msg, masked);
                    }
                }
            }
        }

        for (idx, att) in pack.attachments.iter().enumerate() {
            if let Some(ref content) = att.content {
                match filter.filter(content)? {
                    SensitiveFilterOutcome::Clean => {}
                    SensitiveFilterOutcome::Deny { verbose } => {
                        hit_count += 1;
                        targets.push(format!("attachment:{}", att.title));
                        verbose_all.push_str(&verbose);
                        denied = true;
                    }
                    SensitiveFilterOutcome::Masked { masked, verbose } => {
                        hit_count += 1;
                        targets.push(format!("attachment:{}", att.title));
                        verbose_all.push_str(&verbose);
                        let new_hash = hash64(&masked);
                        let new_bytes = masked.len() as u64;
                        masked_pack.attachments[idx] = ContextAttachment {
                            kind: att.kind.clone(),
                            title: att.title.clone(),
                            content_type: att.content_type.clone(),
                            content: Some(masked),
                            artifact_rel_path: att.artifact_rel_path.clone(),
                            bytes: new_bytes,
                            hash64: new_hash,
                            source: att.source.clone(),
                        };
                    }
                }
            }
        }

        if hit_count == 0 {
            return Ok(RuleVerdict::NoMatch);
        }

        let details = serde_json::json!({
            "hits": hit_count,
            "targets": targets,
            "verbose_truncated": truncate_verbose(&verbose_all, 2000),
        });

        match self.action {
            SensitiveAction::Allow => {
                let decision = PolicyDecision {
                    v: 1,
                    scope: "egress".to_string(),
                    subject: "context_pack".to_string(),
                    status: "warn".to_string(),
                    reason: "sensitive_allow".to_string(),
                    details,
                };
                Ok(RuleVerdict::Verdict(PolicyVerdict::Allow {
                    value: pack.clone(),
                    decision,
                }))
            }
            SensitiveAction::Deny => {
                let decision = PolicyDecision {
                    v: 1,
                    scope: "egress".to_string(),
                    subject: "context_pack".to_string(),
                    status: "blocked".to_string(),
                    reason: "sensitive_deny".to_string(),
                    details,
                };
                Ok(RuleVerdict::Verdict(PolicyVerdict::Deny { decision }))
            }
            SensitiveAction::Mask => {
                let decision = PolicyDecision {
                    v: 1,
                    scope: "egress".to_string(),
                    subject: "context_pack".to_string(),
                    status: "warn".to_string(),
                    reason: "sensitive_masked".to_string(),
                    details,
                };
                Ok(RuleVerdict::Verdict(PolicyVerdict::Allow {
                    value: masked_pack,
                    decision,
                }))
            }
        }
    }
}

