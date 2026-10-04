//! Persistent visual edit locations, independent of viewers and explicit taps.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::CompileView;
use crate::actor::render::RenderActor;

const MAX_RECORD_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PAGES: usize = 50_000;
pub(crate) type ChangePosition = (usize, f64, f64);

#[derive(Clone, Default, Serialize, Deserialize)]
struct Variant {
    page_hashes: Vec<u64>,
    position: Option<ChangePosition>,
}

impl Variant {
    fn valid(&self) -> bool {
        self.page_hashes.len() <= MAX_PAGES
            && self.position.is_none_or(|(page, x, y)| {
                page > 0
                    && page <= self.page_hashes.len()
                    && x.is_finite()
                    && y.is_finite()
                    && x >= 0.0
                    && y >= 0.0
            })
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    schema_version: u32,
    project: String,
    variants: BTreeMap<String, Variant>,
}

struct ChangeStore {
    path: PathBuf,
    record: parking_lot::Mutex<Record>,
}

/// A variant-specific view of one project's persisted visual edit history.
pub(crate) struct ChangeTracker {
    store: Arc<ChangeStore>,
    variant: String,
}

impl ChangeTracker {
    pub fn new(path: PathBuf, project: String, variant: &str) -> io::Result<Self> {
        if !path.is_absolute() || !matches!(variant, "default" | "light" | "dark") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid preview change path or variant",
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("missing change directory"))?;
        std::fs::create_dir_all(parent)?;
        let loaded = (|| -> io::Result<Record> {
            let mut bytes = Vec::new();
            std::fs::File::open(&path)?
                .take(MAX_RECORD_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_RECORD_BYTES {
                return Err(io::Error::other("preview change record is oversized"));
            }
            Ok(serde_json::from_slice(&bytes)?)
        })();
        let record = match loaded {
            Ok(record)
                if record.schema_version == 1
                    && record.project == project
                    && record.variants.len() <= 3
                    && record.variants.iter().all(|(key, value)| {
                        matches!(key.as_str(), "default" | "light" | "dark") && value.valid()
                    }) =>
            {
                record
            }
            Ok(_) => Record {
                schema_version: 1,
                project,
                variants: BTreeMap::new(),
            },
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    log::warn!("Ignoring invalid preview change record: {error}");
                }
                Record {
                    schema_version: 1,
                    project,
                    variants: BTreeMap::new(),
                }
            }
        };
        Ok(Self {
            store: Arc::new(ChangeStore {
                path,
                record: parking_lot::Mutex::new(record),
            }),
            variant: variant.into(),
        })
    }

    pub fn sibling(&self, variant: &str) -> Self {
        Self {
            store: self.store.clone(),
            variant: variant.into(),
        }
    }

    /// Called by the compilation watcher, even when there are no render actors.
    pub fn observe(&self, view: &Arc<dyn CompileView>, previous: Option<&Arc<dyn CompileView>>) {
        let Some(hashes) = view.doc().as_ref().and_then(RenderActor::page_hashes) else {
            return;
        };
        if hashes.len() > MAX_PAGES {
            return;
        }
        let mut record = self.store.record.lock();
        let old = record.variants.get(&self.variant);
        if old.is_some_and(|old| old.page_hashes == hashes) {
            return;
        }
        let changed = old
            .map(|old| RenderActor::changed_pages(&old.page_hashes, &hashes))
            .unwrap_or_default();
        let position = resolve_change(view, previous, &changed);
        record.variants.insert(
            self.variant.clone(),
            Variant {
                page_hashes: hashes,
                position,
            },
        );
        if let Err(error) = self.store.write(&record) {
            log::warn!("Could not persist preview change location: {error}");
        }
    }

    /// Coordinates are usable only with the exact successful document baseline.
    pub fn position(&self, hashes: &[u64]) -> Option<ChangePosition> {
        let record = self.store.record.lock();
        let variant = record.variants.get(&self.variant)?;
        (variant.page_hashes == hashes)
            .then_some(variant.position)
            .flatten()
    }
}

pub(crate) fn resolve_change(
    view: &Arc<dyn CompileView>,
    previous: Option<&Arc<dyn CompileView>>,
    changed: &[usize],
) -> Option<ChangePosition> {
    let first = *changed.first()?;
    if let Some(previous) = previous {
        for position in view.changed_document_positions(previous.as_ref()) {
            let page = position.page.get();
            if changed.contains(&page) {
                return Some((
                    page,
                    position.point.x.to_pt().max(0.0),
                    position.point.y.to_pt().max(0.0),
                ));
            }
        }
    }
    log::debug!(
        "Preview edit navigation fell back to page {first} ({} changed pages)",
        changed.len()
    );
    Some((first, 0.0, 0.0))
}

impl ChangeStore {
    fn write(&self, record: &Record) -> io::Result<()> {
        // The shared record lock serializes writers from both theme compilers.
        let temp = self.path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
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
            serde_json::to_writer(&mut file, record)?;
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
    use tinymist_std::typst::{TypstDocument, TypstPagedDocument};
    use typst::introspection::PagedPosition;
    use typst::layout::{Abs, Point};

    struct View {
        document: TypstDocument,
    }
    impl CompileView for View {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn doc(&self) -> Option<TypstDocument> {
            Some(self.document.clone())
        }
        fn status(&self) -> CompileStatus {
            CompileStatus::CompileSuccess
        }
        fn is_on_saved(&self) -> bool {
            true
        }
        fn is_by_entry_update(&self) -> bool {
            false
        }
        fn changed_document_positions(&self, _: &dyn CompileView) -> Vec<PagedPosition> {
            vec![PagedPosition {
                page: std::num::NonZeroUsize::new(2).unwrap(),
                point: Point::new(Abs::pt(12.0), Abs::pt(34.0)),
            }]
        }
    }

    fn path() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "tinymist-change-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }
    fn document(text: &str) -> Arc<dyn CompileView> {
        let mut result = None;
        tinymist_tests::run_with_sources(text, |verse, _| {
            let doc = typst::compile::<TypstPagedDocument>(&verse.snapshot())
                .output
                .unwrap();
            result = Some(Arc::new(View {
                document: TypstDocument::Paged(Arc::new(doc)),
            }) as Arc<dyn CompileView>);
        });
        result.unwrap()
    }
    fn hashes(view: &Arc<dyn CompileView>) -> Vec<u64> {
        RenderActor::page_hashes(&view.doc().unwrap()).unwrap()
    }

    #[test]
    fn persists_disconnected_edits_and_restores_only_matching_documents() {
        let path = path();
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        let before = document("First\n#pagebreak()\nBefore");
        let after = document("First\n#pagebreak()\nAfter");
        tracker.observe(&before, None);
        assert_eq!(tracker.position(&hashes(&before)), None);
        tracker.observe(&after, Some(&before));
        assert_eq!(tracker.position(&hashes(&after)), Some((2, 12.0, 34.0)));
        assert_eq!(tracker.position(&hashes(&before)), None);
        tracker.observe(&after, Some(&after));
        assert_eq!(tracker.position(&hashes(&after)), Some((2, 12.0, 34.0)));
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        restarted.observe(&after, None);
        assert_eq!(restarted.position(&hashes(&after)), Some((2, 12.0, 34.0)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn restart_with_changed_output_replaces_stale_coordinates() {
        let path = path();
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "default").unwrap();
        let before = document("First\n#pagebreak()\nBefore");
        let after = document("First\n#pagebreak()\nAfter");
        tracker.observe(&before, None);
        tracker.observe(&after, Some(&before));
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "default").unwrap();
        restarted.observe(&before, None);
        assert_eq!(restarted.position(&hashes(&before)), Some((2, 0.0, 0.0)));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn themes_share_persistence_without_sharing_positions() {
        let path = path();
        let light = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        let dark = light.sibling("dark");
        let before = document("First\n#pagebreak()\nBefore");
        let after = document("First\n#pagebreak()\nAfter");
        light.observe(&before, None);
        dark.observe(&after, None);
        light.observe(&after, Some(&before));
        assert_eq!(light.position(&hashes(&after)), Some((2, 12.0, 34.0)));
        assert_eq!(dark.position(&hashes(&after)), None);
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "dark").unwrap();
        assert_eq!(restarted.position(&hashes(&after)), None);
        assert_eq!(
            restarted.sibling("light").position(&hashes(&after)),
            Some((2, 12.0, 34.0))
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_foreign_and_invalid_records_do_not_resume() {
        let path = path();
        let view = document("First");
        for contents in [
            "bad json",
            r#"{"schema_version":1,"project":"other","variants":{}}"#,
            r#"{"schema_version":1,"project":"project","variants":{"light":{"page_hashes":[0],"position":[999,0,0]}}}"#,
        ] {
            std::fs::write(&path, contents).unwrap();
            let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
            tracker.observe(&view, None);
            assert_eq!(tracker.position(&hashes(&view)), None);
        }
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_RECORD_BYTES + 1).unwrap();
        drop(file);
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        tracker.observe(&view, None);
        assert_eq!(tracker.position(&hashes(&view)), None);
        assert!(ChangeTracker::new("relative.json".into(), "project".into(), "light").is_err());
        std::fs::remove_file(path).unwrap();
    }
}
