use crate::domain::{MemoryKind, StructuredMemoryEntry};

/// 構造化メモリをプロンプト用の短いテキストに整形する
pub fn render_memory_context(entries: &[StructuredMemoryEntry], max_entries: usize) -> String {
    if entries.is_empty() || max_entries == 0 {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push("Relevant memory:".to_string());

    for e in entries.iter().take(max_entries) {
        let kind = match e.kind {
            MemoryKind::Profile => "profile",
            MemoryKind::Pattern => "pattern",
            MemoryKind::Preference => "preference",
        };
        let mut topics = e.topics.clone();
        topics.retain(|t| !t.trim().is_empty());
        let topics_str = if topics.is_empty() {
            "".to_string()
        } else {
            format!("[{}]", topics.join(","))
        };
        let mut summary = e.summary.clone();
        if summary.len() > 200 {
            let mut end = 200;
            while end > 0 && !summary.is_char_boundary(end) {
                end -= 1;
            }
            summary.truncate(end);
            summary.push_str("...");
        }
        let line = if topics_str.is_empty() {
            format!("- [{}] {}", kind, summary)
        } else {
            format!("- [{}]{} {}", kind, topics_str, summary)
        };
        lines.push(line);
    }

    lines.join("\n")
}

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

