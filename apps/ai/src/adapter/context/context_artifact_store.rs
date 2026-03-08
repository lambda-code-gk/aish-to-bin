//! コンテキスト添付ファイルの永続化アダプタ

use crate::domain::{hash64, ContextAttachment};
use crate::ports::outbound::ContextArtifactStore;
use common::domain::event::RunId;
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::FileSystem;
use std::sync::Arc;

/// [A-Za-z0-9._-] 以外は '_'。'..' と path separator は除去（パストラバーサル禁止）
fn sanitize_filename(s: &str) -> String {
    let raw: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let no_dots = raw.replace("..", "_");
    let no_sep = no_dots.replace(std::path::MAIN_SEPARATOR, "_");
    let trimmed = no_sep.trim_matches('_');
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else if trimmed.len() > 60 {
        let mut end = 60;
        while end > 0 && !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        trimmed[..end].to_string()
    } else {
        trimmed.to_string()
    }
}

pub struct StdContextArtifactStore {
    fs: Arc<dyn FileSystem>,
}

impl StdContextArtifactStore {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }
}

impl ContextArtifactStore for StdContextArtifactStore {
    fn store(
        &self,
        session_dir: &SessionDir,
        run_id: &RunId,
        attachments: &[ContextAttachment],
    ) -> Result<Vec<ContextAttachment>, Error> {
        let run_id_str: &str = run_id;
        let base_dir = session_dir
            .as_path()
            .join("artifacts")
            .join("context")
            .join(run_id_str);
        self.fs.create_dir_all(&base_dir)?;

        let mut result = Vec::with_capacity(attachments.len());
        for (idx, att) in attachments.iter().enumerate() {
            match att.content {
                None => {
                    result.push(att.clone());
                }
                Some(ref content) => {
                    let h = hash64(content);
                    let safe_title = sanitize_filename(&att.title);
                    let filename = format!("{:04}_{}_{}.txt", idx, att.kind, safe_title);
                    let full_path = base_dir.join(&filename);
                    self.fs.write(&full_path, content)?;

                    let rel_path = format!("artifacts/context/{}/{}", run_id_str, filename);
                    result.push(ContextAttachment {
                        kind: att.kind.clone(),
                        title: att.title.clone(),
                        content_type: att.content_type.clone(),
                        content: None,
                        artifact_rel_path: Some(rel_path),
                        bytes: content.len() as u64,
                        hash64: h,
                        source: att.source.clone(),
                    });
                }
            }
        }
        Ok(result)
    }
}
