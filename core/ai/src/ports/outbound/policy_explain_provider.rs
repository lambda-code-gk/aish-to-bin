use crate::domain::PolicyExplainInfo;
use common::error::Error;

/// policy explain 情報を返すポート
pub trait PolicyExplainProvider: Send + Sync {
    fn explain(&self) -> Result<PolicyExplainInfo, Error>;
}
