use crate::domain::{ContextPack, PolicyVerdict, ToolProfile};
use common::error::Error;
use common::tool::ToolContext;

/// 個々のルールが返す結果（NoMatch or Verdict）
#[derive(Debug, Clone)]
pub enum RuleVerdict<T> {
    /// このルールにはマッチしなかった（次のルールへ）
    NoMatch,
    /// 最終 verdict を返す
    Verdict(PolicyVerdict<T>),
}

/// egress 用ポリシールール
pub trait EgressPolicyRule: Send + Sync {
    fn name(&self) -> &'static str;

    fn evaluate(
        &self,
        pack: &ContextPack,
        non_interactive: bool,
    ) -> Result<RuleVerdict<ContextPack>, Error>;
}

/// tool 呼び出し用ポリシールール
pub trait ToolPolicyRule: Send + Sync {
    fn name(&self) -> &'static str;

    fn evaluate(
        &self,
        tool_name: &str,
        tool_args: &serde_json::Value,
        tool_profile: &ToolProfile,
        tool_ctx: &ToolContext,
        non_interactive: bool,
    ) -> Result<RuleVerdict<ToolContext>, Error>;
}
