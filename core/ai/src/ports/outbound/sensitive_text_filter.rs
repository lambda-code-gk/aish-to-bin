//! 機微情報テキストフィルタの Outbound ポート

use crate::domain::SensitiveFilterOutcome;
use common::error::Error;

pub trait SensitiveTextFilter: Send + Sync {
    fn filter(&self, content: &str) -> Result<SensitiveFilterOutcome, Error>;
}
