//! 責務: memory エントリの重複除去と scope 優先順位の適用のみ。I/O を知らない。

use crate::domain::StructuredMemoryEntry;
use std::collections::HashSet;

/// project 優先で重複除去し、total_limit まで結合する純関数。
///
/// 重複キーは `summary.trim() + "::" + kind` で判定する。
/// project_entries を先に追加し、残り枠に global_entries を追加する。
pub fn merge_memory_entries_project_first(
    project_entries: Vec<StructuredMemoryEntry>,
    global_entries: Vec<StructuredMemoryEntry>,
    total_limit: usize,
) -> Vec<StructuredMemoryEntry> {
    let mut seen = HashSet::new();
    let mut combined = Vec::new();

    for e in project_entries {
        let key = format!("{}::{:?}", e.summary.trim(), e.kind);
        if seen.insert(key) {
            combined.push(e);
        }
        if combined.len() >= total_limit {
            break;
        }
    }

    if combined.len() < total_limit {
        for e in global_entries {
            let key = format!("{}::{:?}", e.summary.trim(), e.kind);
            if seen.insert(key) {
                combined.push(e);
            }
            if combined.len() >= total_limit {
                break;
            }
        }
    }

    combined
}

/// topics を正規化する純関数（trim + 空除去 + lowercase）。
pub fn normalize_topics(topics: &[String]) -> Vec<String> {
    topics
        .iter()
        .filter_map(|t| {
            let s = t.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_ascii_lowercase())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{MemoryKind, MemoryScope};

    fn entry(
        id: &str,
        kind: MemoryKind,
        summary: &str,
        scope: MemoryScope,
    ) -> StructuredMemoryEntry {
        StructuredMemoryEntry {
            id: id.to_string(),
            kind,
            topics: vec!["t".to_string()],
            title: None,
            summary: summary.to_string(),
            body: None,
            confidence: None,
            updated_at: None,
            source_scope: scope,
        }
    }

    #[test]
    fn project_entries_come_first() {
        let p = vec![entry("p1", MemoryKind::Profile, "A", MemoryScope::Project)];
        let g = vec![entry(
            "g1",
            MemoryKind::Preference,
            "B",
            MemoryScope::Global,
        )];
        let result = merge_memory_entries_project_first(p, g, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "p1");
        assert_eq!(result[1].id, "g1");
    }

    #[test]
    fn duplicates_by_summary_and_kind_are_removed() {
        let p = vec![entry(
            "p1",
            MemoryKind::Profile,
            "Same",
            MemoryScope::Project,
        )];
        let g = vec![entry(
            "g1",
            MemoryKind::Profile,
            "Same",
            MemoryScope::Global,
        )];
        let result = merge_memory_entries_project_first(p, g, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "p1");
    }

    #[test]
    fn respects_total_limit() {
        let p = vec![
            entry("p1", MemoryKind::Profile, "A", MemoryScope::Project),
            entry("p2", MemoryKind::Pattern, "B", MemoryScope::Project),
        ];
        let g = vec![entry(
            "g1",
            MemoryKind::Preference,
            "C",
            MemoryScope::Global,
        )];
        let result = merge_memory_entries_project_first(p, g, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn normalize_trims_and_lowercases() {
        let topics = vec![
            " CI ".to_string(),
            "".to_string(),
            "  ".to_string(),
            "Tests".to_string(),
        ];
        let norm = normalize_topics(&topics);
        assert_eq!(norm, vec!["ci", "tests"]);
    }
}
