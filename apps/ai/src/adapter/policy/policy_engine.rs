//! PolicyEngine の標準実装（Phase 5: ordered rule chain + ToolProfile）

use crate::domain::{ContextPack, PolicyChain, PolicyDecision, PolicyVerdict};
use crate::ports::outbound::{PolicyEngine, ToolProfileProvider};
use common::error::Error;
use common::tool::ToolContext;
use std::sync::Arc;

pub struct StdPolicyEngine {
    pub chain: PolicyChain,
    pub tool_profiles: Arc<dyn ToolProfileProvider>,
}

impl StdPolicyEngine {
    pub fn new(chain: PolicyChain, tool_profiles: Arc<dyn ToolProfileProvider>) -> Self {
        Self {
            chain,
            tool_profiles,
        }
    }
}

impl PolicyEngine for StdPolicyEngine {
    fn evaluate_egress_context_pack(
        &self,
        pack: &ContextPack,
        non_interactive: bool,
    ) -> Result<PolicyVerdict<ContextPack>, Error> {
        for rule in &self.chain.egress_rules {
            match rule.evaluate(pack, non_interactive)? {
                crate::domain::RuleVerdict::NoMatch => continue,
                crate::domain::RuleVerdict::Verdict(v) => return Ok(v),
            }
        }
        Ok(PolicyVerdict::Allow {
            value: pack.clone(),
            decision: PolicyDecision {
                v: 1,
                scope: "egress".to_string(),
                subject: "context_pack".to_string(),
                status: "allowed".to_string(),
                reason: "default_allow".to_string(),
                details: serde_json::json!({}),
            },
        })
    }

    fn evaluate_tool_call(
        &self,
        tool_name: &str,
        tool_args: &serde_json::Value,
        tool_ctx: &ToolContext,
        non_interactive: bool,
    ) -> Result<PolicyVerdict<ToolContext>, Error> {
        let profile = self.tool_profiles.get(tool_name);
        for rule in &self.chain.tool_rules {
            match rule.evaluate(tool_name, tool_args, &profile, tool_ctx, non_interactive)? {
                crate::domain::RuleVerdict::NoMatch => continue,
                crate::domain::RuleVerdict::Verdict(v) => return Ok(v),
            }
        }
        Ok(PolicyVerdict::Allow {
            value: tool_ctx.clone(),
            decision: PolicyDecision {
                v: 1,
                scope: "tool".to_string(),
                subject: tool_name.to_string(),
                status: "allowed".to_string(),
                reason: "default_allow".to_string(),
                details: serde_json::json!({}),
            },
        })
    }
}
