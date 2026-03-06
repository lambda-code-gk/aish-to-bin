//! policy explain の標準実装（解決済み値・ルール名・代表例を返すだけ）

use crate::domain::{PolicyExplainExample, PolicyExplainInfo};
use crate::ports::outbound::PolicyExplainProvider;
use common::error::Error;

/// 事前に組み立てた resolved / ルール名 / examples をそのまま返す
pub struct StdPolicyExplainProvider {
    pub resolved: serde_json::Value,
    pub egress_rule_names: Vec<String>,
    pub tool_rule_names: Vec<String>,
    pub examples: Vec<PolicyExplainExample>,
}

impl StdPolicyExplainProvider {
    pub fn new(
        resolved: serde_json::Value,
        egress_rule_names: Vec<String>,
        tool_rule_names: Vec<String>,
        examples: Vec<PolicyExplainExample>,
    ) -> Self {
        Self {
            resolved,
            egress_rule_names,
            tool_rule_names,
            examples,
        }
    }
}

impl PolicyExplainProvider for StdPolicyExplainProvider {
    fn explain(&self) -> Result<PolicyExplainInfo, Error> {
        Ok(PolicyExplainInfo {
            v: 1,
            resolved: self.resolved.clone(),
            egress_rules: self.egress_rule_names.clone(),
            tool_rules: self.tool_rule_names.clone(),
            examples: self.examples.clone(),
        })
    }
}
