use crate::domain::{MemoryKind, MemoryQuery, MemoryScope, StructuredMemoryEntry};
use crate::ports::outbound::ResolveMemoryDir;
use common::error::Error;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 構造化メモリのクエリ/保存用リポジトリ（既存 memory storage 上の論理レイヤ）
pub trait StructuredMemoryRepository: Send + Sync {
    fn query(
        &self,
        scope: MemoryScope,
        query: &MemoryQuery,
    ) -> Result<Vec<StructuredMemoryEntry>, Error>;

    #[allow(dead_code)]
    fn put(&self, scope: MemoryScope, entry: &StructuredMemoryEntry) -> Result<(), Error>;
}

/// ResolveMemoryDir に基づき既存の metadata.json + entries/*.json を読み書きする実装
pub struct StdStructuredMemoryRepository {
    resolve_memory_dir: Arc<dyn ResolveMemoryDir>,
}

impl StdStructuredMemoryRepository {
    pub fn new(resolve_memory_dir: Arc<dyn ResolveMemoryDir>) -> Self {
        Self { resolve_memory_dir }
    }

    fn dir_for_scope(&self, scope: MemoryScope) -> Result<Option<PathBuf>, Error> {
        let (project, global) = self.resolve_memory_dir.resolve()?;
        let dir = match scope {
            MemoryScope::Project => project,
            MemoryScope::Global => Some(global),
        };
        Ok(dir)
    }
}

fn normalize_topic(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

fn list_memory_entry_files(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let entries_dir = dir.join("entries");
    if !entries_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(&entries_dir)
        .map_err(|e| Error::io_msg(format!("read_dir {}: {}", entries_dir.display(), e)))?
    {
        let entry = entry
            .map_err(|e| Error::io_msg(format!("read_dir entry {}: {}", entries_dir.display(), e)))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(files)
}

fn parse_structured_from_content(
    id: String,
    timestamp: String,
    scope: MemoryScope,
    content: &str,
) -> Option<StructuredMemoryEntry> {
    let v: Value = serde_json::from_str(content).ok()?;
    let obj = v.as_object()?;

    let kind_str = obj.get("kind")?.as_str()?;
    let kind = MemoryKind::from_str_case_insensitive(kind_str)?;

    let topics_val = obj.get("topics")?;
    let mut topics = Vec::new();
    if let Some(arr) = topics_val.as_array() {
        for t in arr {
            if let Some(s) = t.as_str() {
                if let Some(norm) = s.trim().strip_prefix('#') {
                    if !norm.trim().is_empty() {
                        topics.push(norm.trim().to_string());
                    }
                } else if !s.trim().is_empty() {
                    topics.push(s.trim().to_string());
                }
            }
        }
    }
    if topics.is_empty() {
        return None;
    }

    let title = obj
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let summary = obj
        .get("summary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| title.clone().unwrap_or_else(|| String::new()));
    if summary.trim().is_empty() {
        return None;
    }
    let body = obj
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let confidence = obj
        .get("confidence")
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);
    let updated_at = obj
        .get("updated_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| Some(timestamp));

    Some(StructuredMemoryEntry {
        id,
        kind,
        topics,
        title,
        summary,
        body,
        confidence,
        updated_at,
        source_scope: scope,
    })
}

impl StdStructuredMemoryRepository {
    fn load_structured_entries(
        &self,
        dir: &Path,
        scope: MemoryScope,
    ) -> Result<Vec<StructuredMemoryEntry>, Error> {
        use crate::domain::MemoryEntry;

        let mut out = Vec::new();
        for path in list_memory_entry_files(dir)? {
            let s = match fs::read_to_string(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let entry: MemoryEntry = match serde_json::from_str(&s) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if let Some(structured) = parse_structured_from_content(
                entry.id.clone(),
                entry.timestamp.clone(),
                scope,
                &entry.content,
            ) {
                out.push(structured);
            }
        }
        Ok(out)
    }
}

impl StructuredMemoryRepository for StdStructuredMemoryRepository {
    fn query(
        &self,
        scope: MemoryScope,
        query: &MemoryQuery,
    ) -> Result<Vec<StructuredMemoryEntry>, Error> {
        let Some(dir) = self.dir_for_scope(scope)? else {
            return Ok(Vec::new());
        };
        let all = self.load_structured_entries(&dir, scope)?;

        let mut kinds_filter: Option<std::collections::HashSet<MemoryKind>> = None;
        if !query.kinds.is_empty() {
            kinds_filter = Some(query.kinds.iter().cloned().collect());
        }

        let normalized_topics: Vec<String> = query
            .topics
            .iter()
            .filter_map(|t| normalize_topic(t))
            .collect();

        let mut filtered = Vec::new();
        'outer: for e in all {
            if let Some(ref set) = kinds_filter {
                if !set.contains(&e.kind) {
                    continue;
                }
            }
            if !normalized_topics.is_empty() {
                let entry_topics: Vec<String> = e
                    .topics
                    .iter()
                    .filter_map(|t| normalize_topic(t))
                    .collect();
                if !normalized_topics
                    .iter()
                    .any(|q| entry_topics.iter().any(|et| et == q))
                {
                    continue 'outer;
                }
            }
            filtered.push(e);
            if filtered.len() >= query.limit {
                break;
            }
        }
        Ok(filtered)
    }

    fn put(&self, scope: MemoryScope, entry: &StructuredMemoryEntry) -> Result<(), Error> {
        use crate::adapter::context::memory_storage;
        use crate::domain::MemoryEntry;

        let Some(dir) = self.dir_for_scope(scope)? else {
            return Err(Error::invalid_argument(
                "memory directory is not configured for the requested scope",
            ));
        };

        let payload = serde_json::json!({
            "kind": entry.kind.as_str(),
            "topics": entry.topics,
            "title": entry.title,
            "summary": entry.summary,
            "body": entry.body,
            "confidence": entry.confidence,
            "updated_at": entry.updated_at,
        });
        let content = payload.to_string();
        let timestamp = common::ports::outbound::now_iso8601();
        let mem_entry = MemoryEntry::new(
            "",
            content,
            "structured",
            entry.topics.clone(),
            entry
                .title
                .clone()
                .unwrap_or_else(|| entry.summary.clone()),
            timestamp,
        );
        let _ = memory_storage::save_entry(&dir, &mem_entry, None)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MemoryEntry;
    use common::ports::outbound::now_iso8601;
    use std::fs;

    struct StubResolveMemoryDir {
        project: Option<PathBuf>,
        global: PathBuf,
    }

    impl ResolveMemoryDir for StubResolveMemoryDir {
        fn resolve(&self) -> Result<(Option<PathBuf>, PathBuf), Error> {
            Ok((self.project.clone(), self.global.clone()))
        }
    }

    fn write_raw_entry(dir: &Path, entry: &MemoryEntry) {
        let entries_dir = dir.join("entries");
        fs::create_dir_all(&entries_dir).unwrap();
        let path = entries_dir.join(format!("{}.json", entry.id));
        let json = serde_json::to_string_pretty(entry).unwrap();
        fs::write(path, json).unwrap();
    }

    #[test]
    fn profile_entry_can_be_saved_and_queried() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("project");
        let global_dir = tmp.path().join("global");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&global_dir).unwrap();

        let resolver = Arc::new(StubResolveMemoryDir {
            project: Some(project_dir.clone()),
            global: global_dir.clone(),
        });
        let repo = StdStructuredMemoryRepository::new(resolver);

        let entry = StructuredMemoryEntry {
            id: "e1".to_string(),
            kind: MemoryKind::Profile,
            topics: vec!["ci".to_string(), "tests".to_string()],
            title: Some("CI uses GitHub Actions".to_string()),
            summary: "CI uses GitHub Actions.".to_string(),
            body: None,
            confidence: Some(0.9),
            updated_at: Some(now_iso8601()),
            source_scope: MemoryScope::Project,
        };

        repo.put(MemoryScope::Project, &entry).unwrap();

        let q = MemoryQuery::new(
            vec!["ci".to_string()],
            vec![MemoryKind::Profile],
            10,
        );
        let results = repo.query(MemoryScope::Project, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, MemoryKind::Profile);
        assert!(results[0].topics.iter().any(|t| t == "ci"));
    }

    #[test]
    fn malformed_or_unstructured_entries_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let global_dir = tmp.path().join("global");
        fs::create_dir_all(&global_dir).unwrap();

        let resolver = Arc::new(StubResolveMemoryDir {
            project: None,
            global: global_dir.clone(),
        });
        let repo = StdStructuredMemoryRepository::new(resolver);

        // unstructured entry (plain text content)
        let e1 = MemoryEntry::new(
            "u1",
            "plain text content",
            "general",
            vec!["ci".to_string()],
            "subject",
            now_iso8601(),
        );
        write_raw_entry(&global_dir, &e1);

        // malformed json
        let e2 = MemoryEntry::new(
            "u2",
            "{ not json }",
            "general",
            vec!["ci".to_string()],
            "subject2",
            now_iso8601(),
        );
        write_raw_entry(&global_dir, &e2);

        let q = MemoryQuery::new(
            vec!["ci".to_string()],
            vec![MemoryKind::Profile],
            10,
        );
        let results = repo.query(MemoryScope::Global, &q).unwrap();
        assert!(results.is_empty());
    }
}

