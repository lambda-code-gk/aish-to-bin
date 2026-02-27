//! PolicyEngine: egress / tool 実行の統一判定ポート

use crate::domain::{ContextPack, PolicyVerdict};
use common::error::Error;
use common::tool::ToolContext;

pub trait PolicyEngine: Send + Sync {
    fn evaluate_egress_context_pack(
        &self,
        pack: &ContextPack,
        non_interactive: bool,
    ) -> Result<PolicyVerdict<ContextPack>, Error>;

    fn evaluate_tool_call(
        &self,
        tool_name: &str,
        tool_args: &serde_json::Value,
        tool_ctx: &ToolContext,
        non_interactive: bool,
    ) -> Result<PolicyVerdict<ToolContext>, Error>;
}
