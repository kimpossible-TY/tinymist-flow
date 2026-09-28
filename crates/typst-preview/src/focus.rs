//! A bounded, local record of the location explicitly selected in preview.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use reflexo_typst::debug_loc::DocumentPosition;
use serde::{Deserialize, Serialize};

use crate::CompileView;

/// A source excerpt from the same compiler snapshot used for hit testing.
#[derive(Debug, Serialize, Deserialize)]
pub struct PreviewSourceContext {
    pub filepath: String,
    /// One-based source line.
    pub line: usize,
    /// One-based source column, using the compiler's source coordinate units.
    pub column: usize,
    pub excerpt_start_line: usize,
    pub excerpt: String,
    pub excerpt_truncated: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FocusRequest {
    #[serde(flatten)]
    pub position: DocumentPosition,
    pub revision: Option<String>,
}

impl FocusRequest {
    pub fn is_valid(&self) -> bool {
        self.position.page_no > 0
            && self.position.x.is_finite()
            && self.position.y.is_finite()
            && self.position.x >= 0.0
            && self.position.y >= 0.0
            && self.revision.as_ref().is_none_or(|r| r.len() <= 64)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct FocusSnapshot {
    pub schema_version: u32,
    pub server_pid: u32,
    pub server_started_at_unix_ms: u64,
    pub sequence: u64,
    pub selected_at_unix_ms: u64,
    pub viewer_id: u64,
    pub status: String,
    pub position: Option<DocumentPosition>,
    pub rendered_revision: Option<String>,
    pub compiled_revision: Option<String>,
    pub source: Option<PreviewSourceContext>,
}

/// One file per preview service; all viewers share its latest selection.
pub(crate) struct FocusStore {
    path: PathBuf,
    started_at: u64,
    sequence: parking_lot::Mutex<u64>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl FocusStore {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "preview focus path must be absolute",
            ));
        }
        let store = Self {
            path,
            started_at: now_ms(),
            sequence: parking_lot::Mutex::new(0),
        };
        // A new service must not present a selection from an earlier process.
        store.write(&FocusSnapshot {
            schema_version: 1,
            server_pid: std::process::id(),
            server_started_at_unix_ms: store.started_at,
            sequence: 0,
            selected_at_unix_ms: 0,
            viewer_id: 0,
            status: "waiting".into(),
            position: None,
            rendered_revision: None,
            compiled_revision: None,
            source: None,
        })?;
        Ok(store)
    }

    pub fn record(
        &self,
        request: FocusRequest,
        view: Option<Arc<dyn CompileView>>,
        viewer_id: u64,
    ) -> io::Result<FocusSnapshot> {
        if !request.is_valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid preview coordinates",
            ));
        }
        let mut sequence = self.sequence.lock();
        let revision = view.as_ref().map(|v| v.revision());
        let (status, source) = match view {
            None => ("not_ready", None),
            Some(_) if request.revision.is_none() || request.revision != revision => {
                ("stale", None)
            }
            Some(view) => match view.preview_source_context(&request.position) {
                Some(source) => ("selected", Some(source)),
                None => ("unmapped", None),
            },
        };
        *sequence += 1;
        let snapshot = FocusSnapshot {
            schema_version: 1,
            server_pid: std::process::id(),
            server_started_at_unix_ms: self.started_at,
            sequence: *sequence,
            selected_at_unix_ms: now_ms(),
            viewer_id,
            status: status.into(),
            position: Some(request.position),
            rendered_revision: request.revision,
            compiled_revision: revision,
            source,
        };
        self.write(&snapshot)?;
        Ok(snapshot)
    }

    fn write(&self, snapshot: &FocusSnapshot) -> io::Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("missing focus directory"))?;
        std::fs::create_dir_all(parent)?;
        let temp = self.path.with_extension(format!(
            "{}.{}.{}.tmp",
            std::process::id(),
            self.started_at,
            snapshot.sequence
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        let result = (|| {
            serde_json::to_writer_pretty(&mut file, snapshot)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            std::fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CompileStatus;
    use tinymist_std::typst::TypstDocument;

    struct View;
    impl CompileView for View {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn doc(&self) -> Option<TypstDocument> {
            None
        }
        fn status(&self) -> CompileStatus {
            CompileStatus::CompileSuccess
        }
        fn is_on_saved(&self) -> bool {
            false
        }
        fn is_by_entry_update(&self) -> bool {
            false
        }
        fn revision(&self) -> String {
            "7".into()
        }
        fn preview_source_context(&self, pos: &DocumentPosition) -> Option<PreviewSourceContext> {
            (pos.x < 100.0).then(|| PreviewSourceContext {
                filepath: "/example/main.typ".into(),
                line: 9,
                column: 2,
                excerpt_start_line: 1,
                excerpt: "선택한 문장".into(),
                excerpt_truncated: false,
            })
        }
    }

    #[test]
    fn selection_is_replaced_on_miss_stale_revision_and_restart() {
        let path = std::env::temp_dir().join(format!(
            "tinymist-focus-{}-{}.json",
            std::process::id(),
            now_ms()
        ));
        let store = FocusStore::new(path.clone()).unwrap();
        let request = |revision: &str, x| FocusRequest {
            position: DocumentPosition {
                page_no: 1,
                x,
                y: 20.0,
            },
            revision: Some(revision.into()),
        };
        let read =
            || serde_json::from_slice::<FocusSnapshot>(&std::fs::read(&path).unwrap()).unwrap();
        store
            .record(request("7", 10.0), Some(Arc::new(View)), 1)
            .unwrap();
        assert_eq!(read().source.unwrap().excerpt, "선택한 문장");
        store
            .record(request("7", 110.0), Some(Arc::new(View)), 2)
            .unwrap();
        assert_eq!(read().status, "unmapped");
        assert!(read().source.is_none());
        store
            .record(request("6", 10.0), Some(Arc::new(View)), 1)
            .unwrap();
        assert_eq!(read().status, "stale");
        assert!(read().source.is_none());
        assert_eq!(read().sequence, 3);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        FocusStore::new(path.clone()).unwrap();
        assert_eq!(read().status, "waiting");
        assert!(read().source.is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_invalid_positions_and_relative_output() {
        assert!(FocusStore::new("relative.json".into()).is_err());
        for (page_no, x, y) in [(0, 0.0, 0.0), (1, -1.0, 1.0), (1, f32::NAN, 0.0)] {
            assert!(
                !FocusRequest {
                    position: DocumentPosition { page_no, x, y },
                    revision: None
                }
                .is_valid()
            );
        }
    }
}
