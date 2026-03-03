use crate::domain::ConfigExplainInfo;
use common::error::Error;

/// config explain 情報を返すポート
pub trait ConfigExplainProvider: Send + Sync {
    fn explain(&self) -> Result<ConfigExplainInfo, Error>;
}
