use crate::adapter::context::render_memory_context::render_memory_context;
use crate::adapter::context::structured_memory_repository::StructuredMemoryRepository;
use crate::domain::{MemoryKind, MemoryQuery, MemoryScope, ResolvedMemoryContext};
use crate::ports::outbound::MemoryContextResolver;
use common::error::Error;
use std::collections::HashSet;
use std::sync::Arc;

pub struct StdMemoryContextResolver {
    repo: Arc<dyn StructuredMemoryRepository>,
    total_limit: usize,
}

impl StdMemoryContextResolver {
    pub fn new(repo: Arc<dyn StructuredMemoryRepository>, total_limit: usize) -> Self {
        Self { repo, total_limit }
    }

    fn normalize_topics(topics: &[String]) -> Vec<String> {
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
}

impl MemoryContextResolver for StdMemoryContextResolver {
    fn resolve(
        &self,
        project_topics: &[String],
        global_topics: &[String],
    ) -> Result<ResolvedMemoryContext, Error> {
        let norm_project = Self::normalize_topics(project_topics);
        let norm_global = Self::normalize_topics(global_topics);

        if norm_project.is_empty() && norm_global.is_empty() {
            return Ok(ResolvedMemoryContext {
                entries: Vec::new(),
                rendered_summary: String::new(),
            });
        }

        let all_kinds = vec![
            MemoryKind::Profile,
            MemoryKind::Pattern,
            MemoryKind::Preference,
        ];

        let project_query =
            MemoryQuery::new(norm_project.clone(), all_kinds.clone(), self.total_limit);
        let mut project_entries = match self.repo.query(MemoryScope::Project, &project_query) {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };

        let global_query = MemoryQuery::new(norm_global.clone(), all_kinds, self.total_limit);
        let mut global_entries = match self.repo.query(MemoryScope::Global, &global_query) {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };

        let mut seen = HashSet::new();
        let mut combined = Vec::new();

        for e in project_entries.drain(..) {
            let key = format!("{}::{:?}", e.summary.trim(), e.kind);
            if seen.insert(key) {
                combined.push(e);
            }
            if combined.len() >= self.total_limit {
                break;
            }
        }
        if combined.len() < self.total_limit {
            for e in global_entries.drain(..) {
                let key = format!("{}::{:?}", e.summary.trim(), e.kind);
                if seen.insert(key) {
                    combined.push(e);
                }
                if combined.len() >= self.total_limit {
                    break;
                }
            }
        }

        let rendered_summary = render_memory_context(&combined, self.total_limit);
        Ok(ResolvedMemoryContext {
            entries: combined,
            rendered_summary,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{MemoryScope, StructuredMemoryEntry};

    struct StubRepo {
        project: Vec<StructuredMemoryEntry>,
        global: Vec<StructuredMemoryEntry>,
    }

    impl StructuredMemoryRepository for StubRepo {
        fn query(
            &self,
            scope: MemoryScope,
            _query: &MemoryQuery,
        ) -> Result<Vec<StructuredMemoryEntry>, Error> {
            Ok(match scope {
                MemoryScope::Project => self.project.clone(),
                MemoryScope::Global => self.global.clone(),
            })
        }

        fn put(&self, _scope: MemoryScope, _entry: &StructuredMemoryEntry) -> Result<(), Error> {
            Ok(())
        }
    }

    fn mk_entry(
        id: &str,
        kind: MemoryKind,
        topics: &[&str],
        summary: &str,
        scope: MemoryScope,
    ) -> StructuredMemoryEntry {
        StructuredMemoryEntry {
            id: id.to_string(),
            kind,
            topics: topics.iter().map(|s| s.to_string()).collect(),
            title: None,
            summary: summary.to_string(),
            body: None,
            confidence: None,
            updated_at: None,
            source_scope: scope,
        }
    }

    #[test]
    fn project_and_global_are_both_used_with_project_first() {
        let repo = StubRepo {
            project: vec![mk_entry(
                "p1",
                MemoryKind::Profile,
                &["ci"],
                "CI uses GitHub Actions.",
                MemoryScope::Project,
            )],
            global: vec![mk_entry(
                "g1",
                MemoryKind::Preference,
                &["workflow"],
                "Prefer suggestion before shell execution.",
                MemoryScope::Global,
            )],
        };
        let resolver = StdMemoryContextResolver::new(Arc::new(repo), 8);
        let ctx = resolver
            .resolve(&vec!["ci".to_string()], &vec!["workflow".to_string()])
            .unwrap();
        assert_eq!(ctx.entries.len(), 2);
        assert_eq!(ctx.entries[0].id, "p1");
        assert_eq!(ctx.entries[1].id, "g1");
        assert!(ctx.rendered_summary.contains("Relevant memory:"));
    }

    #[test]
    fn empty_topics_returns_empty_context() {
        let repo = StubRepo {
            project: Vec::new(),
            global: Vec::new(),
        };
        let resolver = StdMemoryContextResolver::new(Arc::new(repo), 8);
        let ctx = resolver.resolve(&[], &[]).unwrap();
        assert!(ctx.entries.is_empty());
        assert!(ctx.rendered_summary.is_empty());
    }
}
