//! 責務: domain::render_memory_context への委譲のみ。
//!
//! レンダリングロジックは domain/context/memory_render.rs に移動済み。
//! このモジュールは既存コードからの参照互換のために残す。

#[allow(unused_imports)]
pub use crate::domain::context::memory_render::render_memory_context;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{MemoryKind, MemoryScope, StructuredMemoryEntry};

    #[test]
    fn render_empty_returns_empty_string() {
        let rendered = render_memory_context(&[], 4);
        assert!(rendered.is_empty());
    }

    #[test]
    fn render_profile_pattern_preference_as_bullets() {
        let entries = vec![
            StructuredMemoryEntry {
                id: "1".to_string(),
                kind: MemoryKind::Profile,
                topics: vec!["ci".to_string()],
                title: None,
                summary: "CI uses GitHub Actions.".to_string(),
                body: None,
                confidence: None,
                updated_at: None,
                source_scope: MemoryScope::Project,
            },
            StructuredMemoryEntry {
                id: "2".to_string(),
                kind: MemoryKind::Pattern,
                topics: vec!["tests".to_string()],
                title: None,
                summary: "Flaky tests are often in tests/foo.rs.".to_string(),
                body: None,
                confidence: None,
                updated_at: None,
                source_scope: MemoryScope::Global,
            },
        ];
        let rendered = render_memory_context(&entries, 4);
        assert!(rendered.contains("Relevant memory:"));
        assert!(rendered.contains("[profile][ci]"));
        assert!(rendered.contains("[pattern][tests]"));
    }
}
