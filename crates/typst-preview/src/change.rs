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
const SCHEMA_VERSION: u32 = 2;
pub(crate) type ChangePosition = (usize, f64, f64);

#[derive(Clone, Default, Serialize, Deserialize)]
struct Variant {
    page_hashes: Vec<u64>,
    source_fingerprint: Option<String>,
    position: Option<ChangePosition>,
}

impl Variant {
    fn valid(&self) -> bool {
        self.page_hashes.len() <= MAX_PAGES
            && self.source_fingerprint.as_ref().is_none_or(|fingerprint| {
                fingerprint.len() == 32 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
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
                if record.schema_version == SCHEMA_VERSION
                    && record.project == project
                    && record.variants.len() <= 3
                    && record.variants.iter().all(|(key, value)| {
                        matches!(key.as_str(), "default" | "light" | "dark") && value.valid()
                    }) =>
            {
                record
            }
            Ok(_) => Record {
                schema_version: SCHEMA_VERSION,
                project,
                variants: BTreeMap::new(),
            },
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    log::warn!("Ignoring invalid preview change record: {error}");
                }
                Record {
                    schema_version: SCHEMA_VERSION,
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
        let source_fingerprint = view.preview_source_fingerprint();
        let mut record = self.store.record.lock();
        let old = record.variants.get(&self.variant);
        if old.is_some_and(|old| {
            old.page_hashes == hashes
                && old.source_fingerprint == source_fingerprint
                && (previous.is_some() || old.position.is_some())
        }) {
            return;
        }
        let mut position = old.and_then(|old| {
            let same_inputs = old
                .source_fingerprint
                .as_ref()
                .zip(source_fingerprint.as_ref())
                .is_some_and(|(old, new)| old == new);
            if old.page_hashes == hashes || same_inputs {
                // A comment can change source inputs without changing pixels;
                // a renderer update can change page hashes without any edit.
                // Rebase either baseline without replacing its saved location.
                old.position.filter(|(page, _, _)| *page <= hashes.len())
            } else if previous.is_some() {
                let changed = RenderActor::changed_pages(&old.page_hashes, &hashes);
                resolve_change(view, previous, &changed)
            } else {
                // On startup, changed output cannot identify the user's edit.
                // In particular, the first changed page may just be an outline.
                None
            }
        });
        if previous.is_none() && position.is_none() {
            position = view
                .git_changed_document_positions()
                .into_iter()
                .map(|pos| (pos.page.get(), pos.point.x.to_pt(), pos.point.y.to_pt()))
                .filter(|(page, x, y)| {
                    *page <= hashes.len()
                        && x.is_finite()
                        && y.is_finite()
                        && *x >= 0.0
                        && *y >= 0.0
                })
                .min_by(|a, b| {
                    a.0.cmp(&b.0)
                        .then(a.2.total_cmp(&b.2))
                        .then(a.1.total_cmp(&b.1))
                });
        }
        record.variants.insert(
            self.variant.clone(),
            Variant {
                page_hashes: hashes,
                source_fingerprint,
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
    let previous = previous?;
    if view
        .preview_source_fingerprint()
        .zip(previous.preview_source_fingerprint())
        .is_some_and(|(current, previous)| current == previous)
    {
        return None;
    }
    let first = *changed.first()?;
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
        source_fingerprint: Option<String>,
        git_positions: Vec<PagedPosition>,
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
        fn preview_source_fingerprint(&self) -> Option<String> {
            self.source_fingerprint.clone()
        }
        fn git_changed_document_positions(&self) -> Vec<PagedPosition> {
            self.git_positions.clone()
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
        document_with_inputs(text, text)
    }
    fn document_with_inputs(text: &str, inputs: &str) -> Arc<dyn CompileView> {
        let mut result = None;
        tinymist_tests::run_with_sources(text, |verse, _| {
            let doc = typst::compile::<TypstPagedDocument>(&verse.snapshot())
                .output
                .unwrap();
            result = Some(Arc::new(View {
                document: TypstDocument::Paged(Arc::new(doc)),
                source_fingerprint: Some(format!("{:032x}", tinymist_std::hash::hash128(&inputs))),
                git_positions: vec![],
            }) as Arc<dyn CompileView>);
        });
        result.unwrap()
    }
    fn hashes(view: &Arc<dyn CompileView>) -> Vec<u64> {
        RenderActor::page_hashes(&view.doc().unwrap()).unwrap()
    }

    fn without_fingerprint(view: &Arc<dyn CompileView>) -> Arc<dyn CompileView> {
        Arc::new(View {
            document: view.doc().unwrap(),
            source_fingerprint: None,
            git_positions: vec![],
        })
    }

    fn with_git(view: &Arc<dyn CompileView>, positions: &[ChangePosition]) -> Arc<dyn CompileView> {
        Arc::new(View {
            document: view.doc().unwrap(),
            source_fingerprint: view.preview_source_fingerprint(),
            git_positions: positions
                .iter()
                .map(|&(page, x, y)| PagedPosition {
                    page: std::num::NonZeroUsize::new(page).unwrap(),
                    point: Point::new(Abs::pt(x), Abs::pt(y)),
                })
                .collect(),
        })
    }

    #[test]
    fn git_startup_selects_earliest_page_then_top_then_left_and_preserves_saved_position() {
        let path = path();
        let view = document("Cover\n#pagebreak()\nBody\n#pagebreak()\nLater");
        let view = with_git(
            &view,
            &[(3, 0., 0.), (2, 0., 80.), (2, 40., 20.), (2, 10., 20.)],
        );
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        tracker.observe(&view, None);
        assert_eq!(tracker.position(&hashes(&view)), Some((2, 10., 20.)));
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        let older_git = with_git(&view, &[(1, 0., 0.)]);
        restarted.observe(&older_git, None);
        assert_eq!(restarted.position(&hashes(&view)), Some((2, 10., 20.)));
        let edited = with_git(&document("Cover\n#pagebreak()\nEdited"), &[(1, 0., 0.)]);
        restarted.observe(&edited, Some(&view));
        assert_eq!(restarted.position(&hashes(&edited)), Some((2, 12., 34.)));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn git_fallback_handles_empty_and_stale_startup_history() {
        let path = path();
        let view = document("Cover\n#pagebreak()\nBody");
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        tracker.observe(&view, None);
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        let view = with_git(&view, &[(2, 10., 20.)]);
        restarted.observe(&view, None);
        assert_eq!(restarted.position(&hashes(&view)), Some((2, 10., 20.)));
        let stopped_edit = with_git(
            &document("Edited cover\n#pagebreak()\nBody"),
            &[(1, 15., 30.)],
        );
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        restarted.observe(&stopped_edit, None);
        assert_eq!(
            restarted.position(&hashes(&stopped_edit)),
            Some((1, 15., 30.))
        );
        std::fs::remove_file(path).unwrap();
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
    fn restart_with_changed_source_does_not_guess_the_first_changed_page() {
        let path = path();
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "default").unwrap();
        let before = document("First\n#pagebreak()\nBefore");
        let after = document("First\n#pagebreak()\nAfter");
        tracker.observe(&before, None);
        tracker.observe(&after, Some(&before));
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "default").unwrap();
        restarted.observe(&before, None);
        assert_eq!(restarted.position(&hashes(&before)), None);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unchanged_inputs_rebase_render_changes_without_replacing_the_last_edit() {
        let path = path();
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        let before = document("First\n#pagebreak()\nBefore");
        let edited = document("First\n#pagebreak()\nAfter");
        let rerendered = document_with_inputs(
            "First\n#pagebreak()\nRenderer changed appearance",
            "First\n#pagebreak()\nAfter",
        );
        tracker.observe(&before, None);
        tracker.observe(&edited, Some(&before));
        assert_eq!(resolve_change(&rerendered, Some(&edited), &[2]), None);
        tracker.observe(&rerendered, Some(&edited));
        assert_eq!(
            tracker.position(&hashes(&rerendered)),
            Some((2, 12.0, 34.0))
        );

        let restarted = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        restarted.observe(&edited, None);
        assert_eq!(restarted.position(&hashes(&edited)), Some((2, 12.0, 34.0)));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn nonvisual_source_edits_refresh_the_restart_baseline_and_keep_the_last_edit() {
        let path = path();
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        let before = document("First\n#pagebreak()\nBefore");
        let edited = document("First\n#pagebreak()\nAfter");
        let commented = document("// comment\nFirst\n#pagebreak()\nAfter");
        assert_eq!(hashes(&edited), hashes(&commented));
        tracker.observe(&before, None);
        tracker.observe(&edited, Some(&before));
        tracker.observe(&commented, Some(&edited));
        assert_eq!(tracker.position(&hashes(&commented)), Some((2, 12.0, 34.0)));
        let rerendered = document_with_inputs(
            "First\n#pagebreak()\nNew renderer appearance",
            "// comment\nFirst\n#pagebreak()\nAfter",
        );
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        restarted.observe(&rerendered, None);
        assert_eq!(
            restarted.position(&hashes(&rerendered)),
            Some((2, 12.0, 34.0))
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_restart_fallback_records_establish_a_fresh_baseline() {
        let path = path();
        let view = document("First\n#pagebreak()\nBody");
        let record = serde_json::json!({
            "schema_version": 1,
            "project": "project",
            "variants": {
                "light": { "page_hashes": hashes(&view), "position": [2, 0.0, 0.0] },
            },
        });
        std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        tracker.observe(&view, None);
        assert_eq!(tracker.position(&hashes(&view)), None);
        let record: Record = serde_json::from_reader(std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(record.schema_version, SCHEMA_VERSION);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_fingerprints_never_make_changed_restart_output_look_like_an_edit() {
        let path = path();
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "default").unwrap();
        let before = without_fingerprint(&document("First\n#pagebreak()\nBefore"));
        let after = without_fingerprint(&document("First\n#pagebreak()\nAfter"));
        tracker.observe(&before, None);
        tracker.observe(&after, Some(&before));
        assert_eq!(tracker.position(&hashes(&after)), Some((2, 12.0, 34.0)));

        let restarted = ChangeTracker::new(path.clone(), "project".into(), "default").unwrap();
        restarted.observe(&after, None);
        assert_eq!(restarted.position(&hashes(&after)), Some((2, 12.0, 34.0)));
        restarted.observe(&before, None);
        assert_eq!(restarted.position(&hashes(&before)), None);
        restarted.observe(&before, None);
        assert_eq!(restarted.position(&hashes(&before)), None);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unchanged_inputs_do_not_preserve_a_location_beyond_the_new_page_count() {
        let path = path();
        let tracker = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        let before = document("First\n#pagebreak()\nBefore");
        let after = document("First\n#pagebreak()\nAfter");
        tracker.observe(&before, None);
        tracker.observe(&after, Some(&before));
        let rerendered = document_with_inputs("First", "First\n#pagebreak()\nAfter");
        tracker.observe(&rerendered, Some(&after));
        assert_eq!(tracker.position(&hashes(&rerendered)), None);
        let restarted = ChangeTracker::new(path.clone(), "project".into(), "light").unwrap();
        restarted.observe(&rerendered, None);
        assert_eq!(restarted.position(&hashes(&rerendered)), None);
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
            r#"{"schema_version":2,"project":"other","variants":{}}"#,
            r#"{"schema_version":2,"project":"project","variants":{"light":{"page_hashes":[0],"source_fingerprint":null,"position":[999,0,0]}}}"#,
            r#"{"schema_version":2,"project":"project","variants":{"light":{"page_hashes":[0],"source_fingerprint":"invalid","position":null}}}"#,
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
