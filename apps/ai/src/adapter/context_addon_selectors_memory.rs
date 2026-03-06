//! 永続メモリ検索結果を ContextAddon 化するセレクタ

use crate::adapter::memory_storage;
use crate::domain::{hash64, ContextAddon, ContextAttachment, ContextSource};
use crate::ports::outbound::{ContextAddonInput, ContextAddonSelector, ResolveMemoryDir};
use common::error::Error;
use common::msg::Msg;
use std::sync::Arc;

pub struct MemorySelector {
    resolve_memory_dir: Arc<dyn ResolveMemoryDir>,
    limit: usize,
    max_chars_per_entry: usize,
}

impl MemorySelector {
    pub fn new(
        resolve_memory_dir: Arc<dyn ResolveMemoryDir>,
        limit: usize,
        max_chars_per_entry: usize,
    ) -> Self {
        Self {
            resolve_memory_dir,
            limit,
            max_chars_per_entry,
        }
    }

    fn truncate_content(&self, s: &str) -> String {
        if s.len() <= self.max_chars_per_entry {
            s.to_string()
        } else {
            s[..self.max_chars_per_entry].to_string()
        }
    }
}

impl ContextAddonSelector for MemorySelector {
    fn name(&self) -> &str {
        "memory"
    }

    fn select(&self, input: &ContextAddonInput) -> Result<Vec<ContextAddon>, Error> {
        let query_str = match input.query {
            Some(q) => {
                let s: &str = q.as_ref();
                if s.trim().is_empty() {
                    return Ok(vec![]);
                }
                s.to_string()
            }
            None => return Ok(vec![]),
        };

        let (project_dir, global_dir) = self.resolve_memory_dir.resolve()?;
        let entries = memory_storage::search_entries(
            project_dir.as_deref(),
            &global_dir,
            &query_str,
            None,
            self.limit,
            true,
            None,
        )?;

        let mut addons = Vec::new();
        for entry in entries {
            let source_label = entry.source.as_deref().unwrap_or("unknown");
            let subject = if entry.subject.is_empty() {
                entry.id.clone()
            } else {
                entry.subject.clone()
            };
            let content_truncated = self.truncate_content(&entry.content);
            let keywords_str = entry.keywords.join(", ");
            let content_text = format!(
                "Memory({}) id={} subject={}\nkeywords={}\n---\n{}",
                source_label, entry.id, subject, keywords_str, content_truncated
            );
            let priority = 70u32.saturating_add(entry.score.unwrap_or(0) as u32);
            let msg_text = format!("Context: memory\n```text\n{}\n```", content_text);
            let hash = hash64(&content_text);
            let bytes = content_text.len() as u64;
            let ref_id = format!("memory:{}", entry.id);
            let ctx_source = ContextSource {
                kind: "memory".to_string(),
                ref_id: ref_id.clone(),
            };

            addons.push(ContextAddon {
                id: ref_id,
                kind: "memory".to_string(),
                title: subject,
                priority,
                msg: Msg::user(msg_text),
                attachment: Some(ContextAttachment {
                    kind: "memory".to_string(),
                    title: entry.id.clone(),
                    content_type: "text/plain".to_string(),
                    content: Some(content_text),
                    artifact_rel_path: None,
                    bytes,
                    hash64: hash,
                    source: Some(ctx_source.clone()),
                }),
                source: Some(ctx_source),
            });
        }
        Ok(addons)
    }
}
