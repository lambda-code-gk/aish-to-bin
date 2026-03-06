use crate::domain::ConfigSource;
use serde::{Deserialize, Serialize};

/// 出典情報付きの設定値
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resolved<T> {
    pub value: T,
    pub source: ConfigSource,
}

impl<T> Resolved<T> {
    pub fn new(value: T, source: ConfigSource) -> Self {
        Self { value, source }
    }

    /// 値を写像しつつ source を保持する（将来の設定変換用に予約）
    #[allow(dead_code)]
    pub fn map<U, F>(self, f: F) -> Resolved<U>
    where
        F: FnOnce(T) -> U,
    {
        Resolved {
            value: f(self.value),
            source: self.source,
        }
    }
}
