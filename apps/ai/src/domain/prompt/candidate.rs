//! 責務: prompt 構成に用いる候補ソース（Hook / Task / Skill）のドメイン表現のみ。I/O を知らない。
use crate::domain::{PromptSourceKind, ResolvedPromptSource, SourceProvenance};

/// Prompt 構成のための候補ソース。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCandidate {
    pub kind: PromptSourceKind,
    pub name: String,
    pub content: String,
    pub memory_topics: Vec<String>,
    pub preferred_mode: Option<String>,
    pub allowed_tools: Vec<String>,
    pub denied_tools: Vec<String>,
    pub provenance: Option<SourceProvenance>,
}

impl From<ResolvedPromptSource> for PromptCandidate {
    fn from(src: ResolvedPromptSource) -> Self {
        PromptCandidate {
            kind: src.kind,
            name: src.name,
            content: src.content,
            memory_topics: src.memory_topics,
            preferred_mode: src.preferred_mode,
            allowed_tools: src.allowed_tools,
            denied_tools: src.denied_tools,
            provenance: src.provenance,
        }
    }
}

impl From<PromptCandidate> for ResolvedPromptSource {
    fn from(c: PromptCandidate) -> Self {
        ResolvedPromptSource {
            kind: c.kind,
            name: c.name,
            content: c.content,
            memory_topics: c.memory_topics,
            preferred_mode: c.preferred_mode,
            allowed_tools: c.allowed_tools,
            denied_tools: c.denied_tools,
            provenance: c.provenance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_between_candidate_and_resolved() {
        let prov = SourceProvenance::new(
            common::domain::CatalogScope::UserConfig,
            None,
            std::path::PathBuf::from("path"),
            Some("note".to_string()),
        );
        let resolved = ResolvedPromptSource {
            kind: PromptSourceKind::TaskPrompt,
            name: "task".to_string(),
            content: "content".to_string(),
            memory_topics: vec!["topic".to_string()],
            preferred_mode: Some("readonly".to_string()),
            allowed_tools: vec!["read_file".to_string()],
            denied_tools: vec!["write_file".to_string()],
            provenance: Some(prov),
        };
        let cand: PromptCandidate = resolved.clone().into();
        let back: ResolvedPromptSource = cand.into();
        assert_eq!(resolved.kind, back.kind);
        assert_eq!(resolved.name, back.name);
        assert_eq!(resolved.content, back.content);
        assert_eq!(resolved.memory_topics, back.memory_topics);
        assert_eq!(resolved.preferred_mode, back.preferred_mode);
        assert_eq!(resolved.allowed_tools, back.allowed_tools);
        assert_eq!(resolved.denied_tools, back.denied_tools);
        assert_eq!(resolved.provenance, back.provenance);
    }
}

