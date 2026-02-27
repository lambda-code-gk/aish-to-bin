//! コンテキスト添付ファイルの永続化アダプタ

use crate::domain::{hash64, ContextAttachment};
use crate::ports::outbound::ContextArtifactStore;
use common::domain::event::RunId;
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::FileSystem;
use std::sync::Arc;

fn sanitize_filename(s: &str) -> String {
    let raw: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    raw
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
                    let safe_title = if safe_title.len() > 60 {
                        safe_title[..60].to_string()
                    } else {
                        safe_title
                    };
                    let filename = format!(
                        "{:04}_{}_{}.txt",
                        idx, safe_title, h
                    );
                    let full_path = base_dir.join(&filename);
                    self.fs.write(&full_path, content)?;

                    let rel_path = format!(
                        "artifacts/context/{}/{}",
                        run_id_str, filename
                    );
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
