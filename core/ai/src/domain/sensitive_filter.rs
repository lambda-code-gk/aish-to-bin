//! 機微情報フィルタの結果型

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SensitiveFilterOutcome {
    Clean,
    Masked { masked: String, verbose: String },
    Deny { verbose: String },
}
