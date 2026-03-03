use crate::domain::ToolProfile;

/// ツール名から ToolProfile を解決するポート
pub trait ToolProfileProvider: Send + Sync {
    /// 未登録ツールは RequireApproval / capabilities 空 で返す（fail-closed 寄り）
    fn get(&self, tool_name: &str) -> ToolProfile;
}
