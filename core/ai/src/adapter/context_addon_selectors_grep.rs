//! クエリ由来のトークンでプロジェクト走査し、ヒット行を ContextAddon 化するセレクタ

use crate::domain::{hash64, ContextAddon, ContextAttachment, ContextSource};
use crate::ports::outbound::{ContextAddonInput, ContextAddonSelector};
use common::error::Error;
use common::msg::Msg;
use common::ports::outbound::FileSystem;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct GrepHitsSelector {
    fs: Arc<dyn FileSystem>,
    max_files: usize,
    max_hits: usize,
    max_bytes_per_file: u64,
    token_min_len: usize,
    ignore_dirs: Vec<String>,
}

impl GrepHitsSelector {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        max_files: usize,
        max_hits: usize,
        max_bytes_per_file: u64,
        token_min_len: usize,
        ignore_dirs: Vec<String>,
    ) -> Self {
        Self {
            fs,
            max_files,
            max_hits,
            max_bytes_per_file,
            token_min_len,
            ignore_dirs,
        }
    }
}

fn extract_tokens(query: &str, min_len: usize) -> Vec<String> {
    let re_like: Vec<String> = query
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| {
            w.len() >= min_len
                && w.chars()
                    .next()
                    .map_or(false, |c| c.is_ascii_alphabetic() || c == '_')
        })
        .take(3)
        .map(|s| s.to_string())
        .collect();

    if !re_like.is_empty() {
        return re_like;
    }

    let compact: String = query.split_whitespace().collect();
    if compact.len() >= min_len {
        let token: String = compact.chars().take(24).collect();
        vec![token]
    } else {
        vec![]
    }
}

fn should_ignore(path: &Path, ignore_dirs: &[String]) -> bool {
    for component in path.components() {
        if let std::path::Component::Normal(os) = component {
            if let Some(s) = os.to_str() {
                if ignore_dirs.iter().any(|d| d == s) {
                    return true;
                }
            }
        }
    }
    false
}

impl ContextAddonSelector for GrepHitsSelector {
    fn name(&self) -> &str {
        "grep_hits"
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

        let tokens = extract_tokens(&query_str, self.token_min_len);
        if tokens.is_empty() {
            return Ok(vec![]);
        }

        let tokens_lower: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
        let root = input.project_root;
        let mut hits: Vec<String> = Vec::new();
        let mut files_scanned = 0usize;

        let mut queue: VecDeque<PathBuf> = VecDeque::new();
        queue.push_back(root.to_path_buf());

        'outer: while let Some(dir) = queue.pop_front() {
            let entries = match self.fs.read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let mut entries = entries;
            entries.sort();

            for entry_path in entries {
                let rel = entry_path.strip_prefix(root).unwrap_or(&entry_path);
                if should_ignore(rel, &self.ignore_dirs) {
                    continue;
                }
                let meta = match self.fs.metadata(&entry_path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if meta.is_dir() {
                    queue.push_back(entry_path);
                    continue;
                }
                if !meta.is_file() || meta.len() > self.max_bytes_per_file {
                    continue;
                }
                if files_scanned >= self.max_files {
                    break 'outer;
                }
                files_scanned += 1;
                let content = match self.fs.read_to_string(&entry_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let rel_display = rel.display().to_string();
                for (line_num_0, line) in content.lines().enumerate() {
                    let line_lower = line.to_lowercase();
                    if tokens_lower.iter().any(|t| line_lower.contains(t.as_str())) {
                        hits.push(format!("{}:{}: {}", rel_display, line_num_0 + 1, line));
                        if hits.len() >= self.max_hits {
                            break 'outer;
                        }
                    }
                }
            }
        }

        if hits.is_empty() {
            return Ok(vec![]);
        }

        let tokens_joined = tokens.join(", ");
        let header = format!(
            "grep-like hits tokens=[{}] root={}",
            tokens_joined,
            root.display()
        );
        let hits_text = format!("{}\n{}", header, hits.join("\n"));
        let hash = hash64(&hits_text);
        let bytes = hits_text.len() as u64;
        let msg_text = format!("Context: grep hits\n```text\n{}\n```", hits_text);

        let ctx_source = ContextSource {
            kind: "grep".to_string(),
            ref_id: "grep_hits".to_string(),
        };

        Ok(vec![ContextAddon {
            id: "grep_hits".to_string(),
            kind: "grep_hits".to_string(),
            title: "grep hits".to_string(),
            priority: 50,
            msg: Msg::user(msg_text),
            attachment: Some(ContextAttachment {
                kind: "grep_hits".to_string(),
                title: "grep hits".to_string(),
                content_type: "text/plain".to_string(),
                content: Some(hits_text),
                artifact_rel_path: None,
                bytes,
                hash64: hash,
                source: Some(ctx_source.clone()),
            }),
            source: Some(ctx_source),
        }])
    }
}
