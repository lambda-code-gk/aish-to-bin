use crate::domain::PolicyConfig;
use common::error::Error;

/// 設定（policy 用）を提供するポート
pub trait ConfigProvider: Send + Sync {
    fn policy_config(&self) -> Result<PolicyConfig, Error>;
}
