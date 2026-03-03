use crate::domain::{EgressPolicyRule, ToolPolicyRule};
use std::sync::Arc;

/// egress / tool それぞれのルールチェーン
#[derive(Clone)]
pub struct PolicyChain {
    pub egress_rules: Vec<Arc<dyn EgressPolicyRule>>,
    pub tool_rules: Vec<Arc<dyn ToolPolicyRule>>,
}

impl PolicyChain {
    /// ルールチェーン構築用（wiring 等で使用予定）
    #[allow(dead_code)]
    pub fn new(
        egress_rules: Vec<Arc<dyn EgressPolicyRule>>,
        tool_rules: Vec<Arc<dyn ToolPolicyRule>>,
    ) -> Self {
        Self {
            egress_rules,
            tool_rules,
        }
    }
}
