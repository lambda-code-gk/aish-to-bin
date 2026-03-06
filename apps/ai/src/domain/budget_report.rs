//! コンテキスト予算の監査レポート

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetReport {
    pub v: u32,
    pub budget: Budget,
    pub input: BudgetStats,
    pub output: BudgetStats,
    pub decisions: Vec<BudgetDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Budget {
    pub max_messages: usize,
    pub max_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetStats {
    pub message_count: usize,
    pub char_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BudgetDecision {
    pub stage: String,
    pub action: String,
    pub reason: String,
    pub details: serde_json::Value,
}
