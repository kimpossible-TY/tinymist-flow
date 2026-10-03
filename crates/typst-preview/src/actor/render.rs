use std::ops::Range;
use std::sync::Arc;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use reflexo_typst::debug_loc::{DocumentPosition, LspPosition, SourceLocation, SourceSpanOffset};
use reflexo_vec2svg::IncrSvgDocServer;
use tinymist_std::typst::TypstDocument;
use tokio::sync::{broadcast, mpsc};
use typst::layout::{Frame, FrameItem};

use super::{editor::EditorActorRequest, webview::WebviewActorRequest};
use crate::debug_loc::SpanInterner;
use crate::outline::Outline;
use crate::protocol;
use crate::{CompileView, DocToSrcJumpInfo, ResolveSourceLocRequest};

#[derive(Debug, Clone)]
pub enum RenderActorRequest {
    RenderFullLatest,
    RenderIncremental,
    EditorResolveSpanRange(Range<SourceSpanOffset>),
    WebviewResolveFrameLoc(DocumentPosition),
    ResolveSourceLoc(ResolveSourceLocRequest),
}

impl RenderActorRequest {
    pub fn is_full_render(&self) -> bool {
        match self {
            Self::RenderFullLatest => true,
            Self::RenderIncremental => false,
            Self::EditorResolveSpanRange(_) => false,
            Self::ResolveSourceLoc(_) => false,
            Self::WebviewResolveFrameLoc(_) => false,
        }
    }
}

pub struct RenderActor {
    mailbox: broadcast::Receiver<RenderActorRequest>,
    view: Arc<parking_lot::RwLock<Option<Arc<dyn CompileView>>>>,
    renderer: IncrSvgDocServer,
    editor_conn_sender: mpsc::UnboundedSender<EditorActorRequest>,
    svg_sender: mpsc::UnboundedSender<Vec<u8>>,
    webview_sender: broadcast::Sender<WebviewActorRequest>,
    previous_view: Option<Arc<dyn CompileView>>,
    previous_page_hashes: Option<Vec<u64>>,
    has_sent_full: bool,
    focus_enabled: bool,
    change_tracker: Option<Arc<crate::change::ChangeTracker>>,
}

impl RenderActor {
    pub fn new(
        mailbox: broadcast::Receiver<RenderActorRequest>,
        view: Arc<parking_lot::RwLock<Option<Arc<dyn CompileView>>>>,
        editor_conn_sender: mpsc::UnboundedSender<EditorActorRequest>,
        svg_sender: mpsc::UnboundedSender<Vec<u8>>,
        webview_sender: broadcast::Sender<WebviewActorRequest>,
        focus_enabled: bool,
        change_tracker: Option<Arc<crate::change::ChangeTracker>>,
    ) -> Self {
        Self {
            mailbox,
            view,
            renderer: Self::new_renderer(),
            editor_conn_sender,
            svg_sender,
            webview_sender,
            previous_view: None,
            previous_page_hashes: None,
            has_sent_full: false,
            focus_enabled,
            change_tracker,
        }
    }

    fn new_renderer() -> IncrSvgDocServer {
        IncrSvgDocServer::default()
    }

    async fn process_message(&mut self, msg: RenderActorRequest) -> bool {
        log::trace!("RenderActor: received message: {msg:?}");

        let res = msg.is_full_render();
        match msg {
            RenderActorRequest::EditorResolveSpanRange(span_range) => {
                log::debug!("RenderActor: resolving EditorResolveSpanRange: {span_range:?}");

                self.editor_resolve_span_range(span_range);
            }
            RenderActorRequest::WebviewResolveFrameLoc(frame_loc) => {
                log::debug!("RenderActor: resolving WebviewResolveFrameLoc: {frame_loc:?}");
                let spans = self.resolve_span_by_frame_loc(&frame_loc);

                log::debug!("RenderActor: resolved WebviewResolveFrameLoc: {spans:?}");
                // end position is used
                if let Some(spans) = spans {
                    self.editor_resolve_span_range(spans.0..spans.1);
                }
            }
            RenderActorRequest::ResolveSourceLoc(req) => {
                log::debug!("RenderActor: resolving ResolveSourceLoc: {req:?}");

                self.resolve_source_loc(req);
            }
            RenderActorRequest::RenderFullLatest | RenderActorRequest::RenderIncremental => {}
        }

        res
    }

    pub async fn run(mut self) {
        let mut pending_full_render = false;
        loop {
            let mut has_full_render = pending_full_render;
            let mut has_incremental_render = false;
            log::debug!("RenderActor: waiting for message");
            match self.mailbox.recv().await {
                Ok(msg) => {
                    has_incremental_render |= matches!(&msg, RenderActorRequest::RenderIncremental);
                    has_full_render |= self.process_message(msg).await;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    log::info!("RenderActor: no more messages");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    log::info!("RenderActor: lagged message. Some events are dropped");
                }
            }
            // read the queue to empty
            while let Ok(msg) = self.mailbox.try_recv() {
                has_incremental_render |= matches!(&msg, RenderActorRequest::RenderIncremental);
                has_full_render |= self.process_message(msg).await;
            }
            // if a full render is requested, we render the latest document
            // otherwise, we render the incremental changes for only once
            let has_full_render = has_full_render;
            // A demand-driven compiler may not have produced its first document
            // when the viewer asks for `current`. Keep that request so the first
            // successful compile includes the full frame and saved resume hint.
            pending_full_render = has_full_render;
            log::debug!("RenderActor: has_full_render: {has_full_render}");
            let Some(view) = self.view.read().clone() else {
                log::info!("RenderActor: document is not ready");
                continue;
            };
            let Some(document) = view.doc() else {
                log::info!("RenderActor: document is not ready");
                continue;
            };
            pending_full_render = false;

            if self.focus_enabled {
                let hint = format!("focus-revision,{}", view.revision()).into_bytes();
                if self.svg_sender.send(hint).is_err() {
                    break;
                }
            }

            let change_location = if has_full_render || has_incremental_render {
                let page_hashes = Self::page_hashes(&document);
                let changed_pages = self
                    .previous_page_hashes
                    .as_ref()
                    .zip(page_hashes.as_ref())
                    .map(|(old, new)| Self::changed_pages(old, new))
                    .unwrap_or_default();
                let location = if has_incremental_render && !has_full_render {
                    self.change_location(&view, &changed_pages)
                } else if has_full_render && !self.has_sent_full {
                    self.change_tracker.as_ref().and_then(|tracker| {
                        page_hashes
                            .as_ref()
                            .and_then(|hashes| tracker.position(hashes))
                    })
                } else {
                    None
                };
                self.previous_view = Some(view);
                self.previous_page_hashes = page_hashes;
                location
            } else {
                None
            };

            let data = self.render(has_full_render, &document);
            self.has_sent_full |= has_full_render;
            if let Some((page, x, y)) = change_location {
                // Queue the hint ahead of its document delta. The frontend
                // applies it only after that delta has finished rendering.
                let kind = if has_full_render { "resume" } else { "change" };
                let hint = format!("{kind},{page} {x} {y}").into_bytes();
                if self.svg_sender.send(hint).is_err() {
                    log::info!("RenderActor: svg_sender is dropped");
                    break;
                }
            }
            let Ok(_) = self.svg_sender.send(data) else {
                log::info!("RenderActor: svg_sender is dropped");
                break;
            };
        }
        log::info!("RenderActor: exiting")
    }

    fn render(&mut self, has_full_render: bool, document: &TypstDocument) -> Vec<u8> {
        if has_full_render {
            self.render_full(document)
        } else {
            self.render_delta(document)
        }
    }

    pub(crate) fn page_hashes(document: &TypstDocument) -> Option<Vec<u64>> {
        let TypstDocument::Paged(document) = document else {
            return None;
        };
        Some(
            document
                .pages()
                .iter()
                .map(|page| {
                    let mut hasher = DefaultHasher::new();
                    Self::hash_visible_frame(&page.frame, &mut hasher);
                    page.fill.hash(&mut hasher);
                    hasher.finish()
                })
                .collect(),
        )
    }

    fn hash_visible_frame(frame: &Frame, hasher: &mut impl Hasher) {
        frame.size().hash(hasher);
        for (point, item) in frame.items() {
            // Typst's Frame hash also includes source spans and introspection
            // tags. Those can change after an edit without changing pixels.
            match item {
                FrameItem::Group(group) => {
                    0_u8.hash(hasher);
                    point.hash(hasher);
                    group.transform.hash(hasher);
                    group.clip.hash(hasher);
                    Self::hash_visible_frame(&group.frame, hasher);
                }
                FrameItem::Text(text) => {
                    1_u8.hash(hasher);
                    point.hash(hasher);
                    text.font.hash(hasher);
                    text.size.hash(hasher);
                    text.fill.hash(hasher);
                    text.stroke.hash(hasher);
                    text.text.hash(hasher);
                    for glyph in &text.glyphs {
                        glyph.id.hash(hasher);
                        glyph.x_advance.hash(hasher);
                        glyph.x_offset.hash(hasher);
                        glyph.y_advance.hash(hasher);
                        glyph.y_offset.hash(hasher);
                    }
                }
                FrameItem::Shape(shape, _) => {
                    2_u8.hash(hasher);
                    point.hash(hasher);
                    shape.hash(hasher);
                }
                FrameItem::Image(image, size, _) => {
                    3_u8.hash(hasher);
                    point.hash(hasher);
                    image.hash(hasher);
                    size.hash(hasher);
                }
                FrameItem::Link(..) | FrameItem::Tag(..) => {}
            }
        }
    }

    pub(crate) fn changed_pages(old: &[u64], new: &[u64]) -> Vec<usize> {
        let mut changed: Vec<_> = new
            .iter()
            .enumerate()
            .filter_map(|(index, hash)| (old.get(index) != Some(hash)).then_some(index + 1))
            .collect();
        if changed.is_empty() && old.len() > new.len() && !new.is_empty() {
            changed.push(new.len());
        }
        changed
    }

    fn change_location(
        &self,
        view: &Arc<dyn CompileView>,
        changed_pages: &[usize],
    ) -> Option<(usize, f64, f64)> {
        crate::change::resolve_change(view, self.previous_view.as_ref(), changed_pages)
    }

    #[typst_macros::time]
    fn render_full(&mut self, document: &TypstDocument) -> Vec<u8> {
        let mut renderer = Self::new_renderer();
        let delta = renderer.pack_delta(document);
        self.renderer = renderer;
        match protocol::full_current_frame_from_delta(&delta) {
            Some(frame) => frame,
            None => {
                log::warn!("fresh preview renderer did not produce a diff-v1 frame");
                delta
            }
        }
    }

    #[typst_macros::time]
    fn render_delta(&mut self, document: &TypstDocument) -> Vec<u8> {
        self.renderer.pack_delta(document)
    }

    fn view(&self) -> Option<Arc<dyn CompileView>> {
        self.view.read().clone()
    }

    fn editor_resolve_span_range(&self, span_range: Range<SourceSpanOffset>) -> Option<()> {
        let req = EditorActorRequest::DocToSrcJump(self.resolve_span_range(span_range)?);
        let _ = self.editor_conn_sender.send(req);

        Some(())
    }

    fn resolve_span_range(&self, range: Range<SourceSpanOffset>) -> Option<DocToSrcJumpInfo> {
        let view = self.view()?;
        // Resolves FileLoC of start, end, and the element wide
        let st_res = view.resolve_span(range.start.span, Some(range.start.offset));
        let ed_res = view.resolve_span(range.end.span, Some(range.end.offset));
        let elem_res = view.resolve_span(range.end.span, None);

        // Combines the result of start and end
        let range_res = match (st_res, ed_res) {
            (Some(st), Some(ed)) => {
                if st.filepath == ed.filepath
                    && matches!((&st.start, &st.end), (Some(x), Some(y)) if x <= y)
                {
                    Some(DocToSrcJumpInfo {
                        filepath: st.filepath,
                        start: st.start,
                        end: ed.start,
                    })
                } else {
                    Some(ed)
                }
            }
            (Some(info), None) | (None, Some(info)) => Some(info),
            (None, None) => None,
        };

        // Account for the case where the start and end are out of order.
        //
        // This could happen because typst supports scripting, which makes text out of
        // order
        let range_res = {
            let mut range_res = range_res;
            if let Some(info) = &mut range_res
                && let Some((x, y)) = info.start.zip(info.end)
                && y <= x
            {
                std::mem::swap(&mut info.start, &mut info.end);
            }

            range_res
        };

        // Restricts the range to the element's range
        match (elem_res, range_res) {
            (Some(elem), Some(mut rng)) if elem.filepath == rng.filepath => {
                // Account for the case where the element's range is out of order.
                let elem_start = elem.start.or(elem.end);
                let elem_end = elem.end.or(elem_start);

                // Account for the case where the range is out of order.
                let rng_start = rng.start.or(rng.end);
                let rng_end = rng.end.or(rng_start);

                if let Some((((u, inner_u), inner_v), v)) =
                    elem_start.zip(rng_start).zip(rng_end).zip(elem_end)
                {
                    rng.start = Some(inner_u.max(u).min(v));
                    rng.end = Some(inner_v.max(u).min(v));
                }
                Some(rng)
            }
            (.., Some(info)) | (Some(info), None) => Some(info),
            (None, None) => None,
        }
    }

    fn resolve_source_loc(&self, req: ResolveSourceLocRequest) -> Option<()> {
        // todo: change name to resolve resolve src position
        let info = self
            .view()?
            .resolve_document_position(crate::Location::Src(SourceLocation {
                filepath: req.filepath.to_string_lossy().to_string(),
                pos: LspPosition {
                    line: req.line,
                    character: req.character,
                },
            }));

        if info.is_empty() {
            return None;
        }

        let _ = self.webview_sender.send(WebviewActorRequest::SrcToDocJump(
            info.into_iter()
                .map(|info| DocumentPosition {
                    page_no: info.page.into(),
                    x: info.point.x.to_pt() as f32,
                    y: info.point.y.to_pt() as f32,
                })
                .collect(),
        ));

        Some(())
    }

    /// Gets the span range of the given frame loc.
    pub fn resolve_span_by_frame_loc(
        &mut self,
        pos: &DocumentPosition,
    ) -> Option<(SourceSpanOffset, SourceSpanOffset)> {
        let view = self.view.read();
        view.as_ref()?.resolve_frame_loc(pos)
    }
}

pub struct OutlineRenderActor {
    signal: broadcast::Receiver<RenderActorRequest>,
    document: Arc<parking_lot::RwLock<Option<Arc<dyn CompileView>>>>,
    editor_tx: mpsc::UnboundedSender<EditorActorRequest>,

    span_interner: SpanInterner,
}

impl OutlineRenderActor {
    pub fn new(
        signal: broadcast::Receiver<RenderActorRequest>,
        document: Arc<parking_lot::RwLock<Option<Arc<dyn CompileView>>>>,
        editor_tx: mpsc::UnboundedSender<EditorActorRequest>,
        span_interner: SpanInterner,
    ) -> Self {
        Self {
            signal,
            document,
            editor_tx,
            span_interner,
        }
    }

    pub async fn run(mut self) {
        loop {
            log::debug!("OutlineRenderActor: waiting for message");
            match self.signal.recv().await {
                Ok(msg) => {
                    log::debug!("OutlineRenderActor: received message: {msg:?}");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    log::info!("OutlineRenderActor: no more messages");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    log::info!("OutlineRenderActor: lagged message. Some events are dropped");
                }
            }
            // read the queue to empty
            while self.signal.try_recv().is_ok() {}
            // if a full render is requested, we render the latest document
            // otherwise, we render the incremental changes for only once
            let Some(document) = self.document.read().as_ref().and_then(|view| view.doc()) else {
                log::info!("OutlineRenderActor: document is not ready");
                continue;
            };
            let data = self.outline(&document).await;
            log::debug!("OutlineRenderActor: sending outline");
            let Ok(_) = self.editor_tx.send(EditorActorRequest::Outline(data)) else {
                log::info!("OutlineRenderActor: outline_sender is dropped");
                break;
            };
        }
        log::info!("OutlineRenderActor: exiting")
    }

    async fn outline(&self, document: &TypstDocument) -> Outline {
        self.span_interner
            .with_writer(|interner| {
                interner.reset();
                crate::outline::outline(interner, document)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CompileStatus;
    use crate::change::ChangeTracker;
    use tinymist_std::typst::TypstPagedDocument;

    struct View(TypstDocument);

    impl CompileView for View {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn doc(&self) -> Option<TypstDocument> {
            Some(self.0.clone())
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
    }

    fn document(source: &str) -> Arc<dyn CompileView> {
        tinymist_tests::run_with_sources(source, |verse, _| {
            let document = typst::compile::<TypstPagedDocument>(&verse.snapshot())
                .output
                .unwrap();
            Arc::new(View(TypstDocument::Paged(Arc::new(document)))) as Arc<dyn CompileView>
        })
    }

    #[tokio::test]
    async fn current_before_first_compile_preserves_full_frame_and_resume_order() {
        let path = std::env::temp_dir().join(format!(
            "tinymist-pending-current-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let tracker =
            Arc::new(ChangeTracker::new(path.clone(), "project".into(), "light").unwrap());
        let before = document("First\n#pagebreak()\nBefore");
        let after = document("First\n#pagebreak()\nAfter");
        tracker.observe(&before, None);
        tracker.observe(&after, Some(&before));

        let (signal, mailbox) = broadcast::channel(16);
        let view = Arc::new(parking_lot::RwLock::new(None));
        let (editor, _editor_rx) = mpsc::unbounded_channel();
        let (svg, mut frames) = mpsc::unbounded_channel();
        let (webview, _webview_rx) = broadcast::channel(16);
        let actor = RenderActor::new(
            mailbox,
            view.clone(),
            editor,
            svg,
            webview,
            false,
            Some(tracker),
        );
        let task = tokio::spawn(actor.run());
        signal.send(RenderActorRequest::RenderFullLatest).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !signal.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("renderer should consume the initial current request without a document");
        assert!(frames.try_recv().is_err());

        *view.write() = Some(after);
        signal.send(RenderActorRequest::RenderIncremental).unwrap();
        let hint = tokio::time::timeout(std::time::Duration::from_secs(2), frames.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hint, b"resume,2 0 0");
        let frame = tokio::time::timeout(std::time::Duration::from_secs(2), frames.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(frame.starts_with(protocol::NEW_PREFIX));
        assert!(frame.len() > protocol::NEW_PREFIX.len());
        drop(signal);
        task.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
