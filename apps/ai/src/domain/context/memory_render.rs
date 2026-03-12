//! 責務: StructuredMemoryEntry をプロンプト用テキストに変換するのみ。I/O を知らない。

use crate::domain::{MemoryKind, StructuredMemoryEntry};

const SUMMARY_MAX_CHARS: usize = 200;

/// UTF-8 文字境界を考慮して文字列を切り詰める。
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn kind_label(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Profile => "profile",
        MemoryKind::Pattern => "pattern",
        MemoryKind::Preference => "preference",
    }
}

/// memory エントリ列をプロンプト用テキストに整形する純関数。
pub fn render_memory_context(entries: &[StructuredMemoryEntry], max_entries: usize) -> String {
    if entries.is_empty() || max_entries == 0 {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push("Relevant memory:".to_string());

    for e in entries.iter().take(max_entries) {
        let label = kind_label(e.kind);

        let mut topics = e.topics.clone();
        topics.retain(|t| !t.trim().is_empty());
        let topics_str = if topics.is_empty() {
            "".to_string()
        } else {
            format!("[{}]", topics.join(","))
        };

        let summary = if e.summary.len() > SUMMARY_MAX_CHARS {
            let truncated = truncate_at_char_boundary(&e.summary, SUMMARY_MAX_CHARS);
            format!("{}...", truncated)
        } else {
            e.summary.clone()
        };

        let line = if topics_str.is_empty() {
            format!("- [{}] {}", label, summary)
        } else {
            format!("- [{}]{} {}", label, topics_str, summary)
        };
        lines.push(line);
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MemoryScope;

    fn entry(kind: MemoryKind, topics: &[&str], summary: &str) -> StructuredMemoryEntry {
        StructuredMemoryEntry {
            id: "1".to_string(),
            kind,
            topics: topics.iter().map(|s| s.to_string()).collect(),
            title: None,
            summary: summary.to_string(),
            body: None,
            confidence: None,
            updated_at: None,
            source_scope: MemoryScope::Project,
        }
    }

    #[test]
    fn empty_returns_empty() {
        assert!(render_memory_context(&[], 4).is_empty());
    }

    #[test]
    fn renders_bullets_with_kind_and_topics() {
        let entries = vec![
            entry(MemoryKind::Profile, &["ci"], "CI uses GitHub Actions."),
            entry(MemoryKind::Pattern, &["tests"], "Flaky tests."),
        ];
        let rendered = render_memory_context(&entries, 4);
        assert!(rendered.contains("Relevant memory:"));
        assert!(rendered.contains("[profile][ci]"));
        assert!(rendered.contains("[pattern][tests]"));
    }

    #[test]
    fn truncates_long_summaries() {
        let long = "a".repeat(300);
        let entries = vec![entry(MemoryKind::Profile, &["x"], &long)];
        let rendered = render_memory_context(&entries, 4);
        assert!(rendered.contains("..."));
        assert!(rendered.len() < 300 + 50);
    }

    #[test]
    fn respects_max_entries() {
        let entries = vec![
            entry(MemoryKind::Profile, &["a"], "First"),
            entry(MemoryKind::Pattern, &["b"], "Second"),
            entry(MemoryKind::Preference, &["c"], "Third"),
        ];
        let rendered = render_memory_context(&entries, 2);
        assert!(rendered.contains("First"));
        assert!(rendered.contains("Second"));
        assert!(!rendered.contains("Third"));
    }
}
