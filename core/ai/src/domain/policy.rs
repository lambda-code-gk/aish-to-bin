//! PolicyEngine 判定結果の型

use serde::{Deserialize, Serialize};

/// 統一的な判定結果（egress / tool 共通）
#[derive(Debug, Clone)]
pub enum PolicyVerdict<T> {
    Allow {
        value: T,
        decision: PolicyDecision,
    },
    RequireApproval {
        value: T,
        decision: PolicyDecision,
        prompt: String,
    },
    Deny {
        decision: PolicyDecision,
    },
}

/// 判定記録（event payload / BudgetReport.decisions と互換の構造体）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyDecision {
    pub v: u32,
    pub scope: String,
    pub subject: String,
    pub status: String,
    pub reason: String,
    pub details: serde_json::Value,
}

impl PolicyDecision {
    pub fn to_event_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "v": self.v,
            "scope": self.scope,
            "subject": self.subject,
            "status": self.status,
            "reason": self.reason,
            "details": self.details,
        })
    }
}
