use serde::{Deserialize, Serialize};

/// 論理メモリの種類（profile / pattern / preference）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Profile,
    Pattern,
    Preference,
}

impl MemoryKind {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryKind::Profile => "profile",
            MemoryKind::Pattern => "pattern",
            MemoryKind::Preference => "preference",
        }
    }

    pub fn from_str_case_insensitive(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "profile" => Some(MemoryKind::Profile),
            "pattern" => Some(MemoryKind::Pattern),
            "preference" => Some(MemoryKind::Preference),
            _ => None,
        }
    }
}

