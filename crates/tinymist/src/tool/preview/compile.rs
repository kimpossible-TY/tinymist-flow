//! Document preview tool for Typst

use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::Path;

use reflexo::debug_loc::SourceSpanOffset;
use reflexo_typst::{error::prelude::*, Bytes, Error, TypstDocument};
use tinymist_preview::{
    CompileStatus, DocToSrcJumpInfo, EditorServer, Location, MemoryFiles, MemoryFilesShort,
};
use tinymist_project::LspCompiledArtifact;
use tinymist_query::{jump_from_click, jump_from_cursor};
use typst::introspection::{MetadataElem, PagedPosition as Position, Tag};
use typst::layout::{Abs, Frame, FrameItem, Point};
use typst::model::{Destination, OutlineElem, OutlineEntry};
use typst::syntax::{FileId, LinkedNode, Source, Span, SyntaxKind, VirtualRoot};
use typst::World;
use typst_shim::syntax::LinkedNodeExt;

use crate::project::{LspInterrupt, ProjectClient, ProjectInsId};
use crate::world::vfs::{notify::MemoryEvent, FileChangeSet};
use crate::*;

/// The compiler's view of the a preview task (server).
pub struct ProjectPreviewHandler {
    /// The project id.
    pub project_id: ProjectInsId,
    /// The connection to the compiler compiling projects (language server).
    pub client: Box<dyn ProjectClient>,
}

impl ProjectPreviewHandler {
    /// Pause compilation while no viewer consumes this preview variant.
    pub fn set_viewer_demand(&self, active: bool) {
        self.client
            .interrupt(LspInterrupt::SetDemand(self.project_id.clone(), active));
    }

    /// Requests the compiler to compile the project.
    pub fn flush_compile(&self) {
        let _ = self.project_id;
        self.client
            .interrupt(LspInterrupt::Compile(self.project_id.clone()));
    }

    /// Requests the compiler to settle the project.
    pub fn settle(&self) -> Result<(), Error> {
        self.client
            .interrupt(LspInterrupt::Settle(self.project_id.clone()));
        Ok(())
    }

    /// Requests the compiler to unpin the primary project.
    pub fn unpin_primary(&self) {
        self.client.server_event(ServerEvent::UnpinPrimaryByPreview);
    }
}

impl EditorServer for ProjectPreviewHandler {
    async fn update_memory_files(
        &self,
        files: MemoryFiles,
        reset_shadow: bool,
    ) -> Result<(), Error> {
        // todo: is it safe to believe that the path is normalized?
        let files = FileChangeSet::new_inserts(
            files
                .files
                .into_iter()
                .map(|(path, content)| {
                    // todo: cloning PathBuf -> Arc<Path>
                    (path.into(), Ok(Bytes::from_string(content)).into())
                })
                .collect(),
        );

        let intr = LspInterrupt::Memory(if reset_shadow {
            MemoryEvent::Sync(files)
        } else {
            MemoryEvent::Update(files)
        });
        self.client.interrupt(intr);

        Ok(())
    }

    async fn remove_memory_files(&self, files: MemoryFilesShort) -> Result<(), Error> {
        // todo: is it safe to believe that the path is normalized?
        let files = FileChangeSet::new_removes(files.files.into_iter().map(From::from).collect());
        self.client
            .interrupt(LspInterrupt::Memory(MemoryEvent::Update(files)));

        Ok(())
    }
}

/// The preview's view of the compiled artifact.
pub struct PreviewCompileView {
    /// The compiled artifact.
    pub art: LspCompiledArtifact,
}

impl tinymist_preview::CompileView for PreviewCompileView {
    fn revision(&self) -> String {
        self.art.world().revision().get().to_string()
    }

    fn preview_document_id(&self) -> Option<String> {
        let world = self.art.world();
        let root = world.entry_state().workspace_root()?;
        let entry = world.path_for_id(world.main_id()?).ok()?.to_err().ok()?;
        Some(format!(
            "{:032x}",
            tinymist_std::hash::hash128(&(root, entry))
        ))
    }

    fn preview_source_fingerprint(&self) -> Option<String> {
        let world = self.art.world();
        let mut inputs = Vec::new();
        for id in self.art.depended_files().iter().copied() {
            // Source reads use the compiled snapshot, including unsaved edits.
            // Binary dependencies such as images contribute their bytes too.
            let content = match world.source(id) {
                Ok(source) => tinymist_std::hash::hash128(&source.text()),
                Err(_) => tinymist_std::hash::hash128(&world.file(id).ok()?),
            };
            inputs.push((
                match id.root() {
                    VirtualRoot::Project => None,
                    VirtualRoot::Package(package) => Some(package.to_string()),
                },
                id.vpath().get_with_slash().to_owned(),
                content,
            ));
        }
        // FileId intern numbers and dependency discovery order are process-local.
        inputs.sort_unstable();
        inputs.dedup();
        Some(format!("{:032x}", tinymist_std::hash::hash128(&inputs)))
    }

    fn highlight_preview_selection(
        &self,
        start: &reflexo::debug_loc::DocumentPosition,
        selection: &tinymist_preview::PreviewTextSelection,
    ) -> tinymist_preview::PreviewHighlightStatus {
        use tinymist_preview::PreviewHighlightStatus as Status;
        let resolve = || {
            let (first, _) = self.resolve_frame_loc(start)?;
            let (last, _) = self.resolve_frame_loc(&selection.end)?;
            let id = first.span.id()?;
            if last.span.id()? != id || crate::world::vfs::WorkspaceResolver::is_package_file(id) {
                return None;
            }
            let world = self.art.world();
            let source = world.source(id).ok()?;
            let first_range = source.find(first.span)?.range();
            let last_range = source.find(last.span)?.range();
            let first_offset =
                first_range.start + first.offset.min(first_range.len().saturating_sub(1));
            let last_offset =
                last_range.start + last.offset.min(last_range.len().saturating_sub(1));
            let function = typst_shim::eval::eval_compat(world, &source)
                .ok()
                .and_then(|module| {
                    module
                        .scope()
                        .get("highlighted")
                        .map(|binding| matches!(binding.read(), typst::foundations::Value::Func(_)))
                })
                .filter(|callable| *callable)
                .map_or(super::highlight::HighlightFunction::Standard, |_| {
                    super::highlight::HighlightFunction::MathAware
                });
            let TypstDocument::Paged(doc) = self.art.doc.as_ref()? else {
                return None;
            };
            let mut glyphs = vec![];
            for page in doc.pages() {
                collect_source_glyphs(&page.frame, &source, &mut glyphs);
            }
            let range = super::highlight::selection_range(
                &source,
                first_offset,
                last_offset,
                &selection.text,
                &glyphs,
                function,
            );
            let path = world.path_for_id(id).ok()?.to_err().ok()?;
            let path = std::fs::canonicalize(path).ok()?;
            let root = std::fs::canonicalize(world.entry_state().workspace_root()?).ok()?;
            if !path.starts_with(root) {
                return None;
            }
            // A custom helper may fail for a particular body or be unavailable
            // at this lexical position. Validate in an isolated world before
            // committing an edit to the user's document.
            let compile = |updated: String| {
                use crate::world::base::ShadowApi;
                let mut candidate = world.clone();
                // Cloning shares the completed world's frozen source database.
                // Detach it so the compile reads the candidate shadow source.
                candidate.take_db();
                candidate
                    .map_shadow_by_id(id, Bytes::from_string(updated))
                    .ok()?;
                typst::compile::<tinymist_std::typst::TypstPagedDocument>(&candidate)
                    .output
                    .ok()
            };
            let range = if let Some(range) = range {
                range
            } else {
                // References made by imported helpers lose their call-site
                // glyph origins. Verify each whole-call candidate by rendering
                // its actual output between invisible metadata markers.
                let mut verified = None;
                for range in super::highlight::reference_ranges(
                    &source,
                    first_offset,
                    last_offset,
                    &glyphs,
                    function,
                ) {
                    let token = format!(
                        "flow-preview-highlight-{:032x}",
                        tinymist_std::hash::hash128(&(source.text(), &range))
                    );
                    let updated = super::highlight::marked_highlight_source(
                        &source,
                        range.clone(),
                        function,
                        &token,
                    );
                    let Some(document) = compile(updated) else {
                        continue;
                    };
                    let Some(rendered) = marked_highlight_text(&document, &token) else {
                        continue;
                    };
                    if super::highlight::rendered_reference_matches(
                        &rendered,
                        &selection.text,
                        matches!(function, super::highlight::HighlightFunction::MathAware),
                    ) {
                        if verified.is_some() {
                            return None;
                        }
                        verified = Some(range);
                    }
                }
                verified?
            };
            compile(super::highlight::highlighted_source(
                &source,
                range.clone(),
                function,
            ))?;
            Some((source, path, range, function))
        };
        let Some((source, path, range, function)) = resolve() else {
            return Status::Unmapped;
        };
        super::highlight::write_highlight(&path, &source, range, function)
    }

    fn preview_source_context(
        &self,
        pos: &reflexo::debug_loc::DocumentPosition,
    ) -> Option<tinymist_preview::PreviewSourceContext> {
        let (span, _) = self.resolve_frame_loc(pos)?;
        let location = self.resolve_span(span.span, Some(span.offset))?;
        let (line, column) = location.start?;
        let source = self.art.world().source(span.span.id()?).ok()?;
        let start = line.saturating_sub(8);
        let excerpt = source
            .text()
            .lines()
            .skip(start)
            .take(17)
            .collect::<Vec<_>>()
            .join("\n");
        let excerpt_truncated = excerpt.chars().count() > 8000;
        Some(tinymist_preview::PreviewSourceContext {
            filepath: location.filepath,
            line: line + 1,
            column: column + 1,
            excerpt_start_line: start + 1,
            excerpt: excerpt.chars().take(8000).collect(),
            excerpt_truncated,
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn doc(&self) -> Option<TypstDocument> {
        self.art.doc.clone()
    }

    fn status(&self) -> CompileStatus {
        match self.art.doc {
            Some(_) => CompileStatus::CompileSuccess,
            None => CompileStatus::CompileError,
        }
    }

    fn is_on_saved(&self) -> bool {
        self.art.snap.signal.by_fs_events
    }

    fn is_by_entry_update(&self) -> bool {
        self.art.snap.signal.by_entry_update
    }

    fn resolve_source_span(&self, loc: Location) -> Option<SourceSpanOffset> {
        let world = self.art.world();
        let Location::Src(loc) = loc;

        let source_id = world.id_for_path(Path::new(&loc.filepath))?;

        let source = world.source(source_id).ok()?;
        let cursor = source
            .lines()
            .line_column_to_byte(loc.pos.line as usize, loc.pos.character as usize)?;

        let node = LinkedNode::new(source.root()).leaf_at_compat(cursor)?;
        if !matches!(node.kind(), SyntaxKind::Text | SyntaxKind::MathText) {
            return None;
        }
        let span = node.span();
        // todo: unicode char
        let offset = cursor.saturating_sub(node.offset());

        Some(SourceSpanOffset { span, offset })
    }

    // todo: use vec2bbox to handle bbox correctly
    fn resolve_frame_loc(
        &self,
        pos: &reflexo::debug_loc::DocumentPosition,
    ) -> Option<(SourceSpanOffset, SourceSpanOffset)> {
        let TypstDocument::Paged(doc) = self.doc()? else {
            return None;
        };
        let world = self.art.world();

        let page = pos.page_no.checked_sub(1)?;
        let page = doc.pages().get(page)?;

        let click = Point::new(Abs::pt(pos.x as f64), Abs::pt(pos.y as f64));
        jump_from_click(world, &page.frame, click)
    }

    fn resolve_document_position(&self, loc: Location) -> Vec<Position> {
        let world = self.art.world();
        let Location::Src(src_loc) = loc;

        let line = src_loc.pos.line as usize;
        let column = src_loc.pos.character as usize;

        let doc = self.art.success_doc();
        let Some(doc) = doc.as_ref() else {
            return vec![];
        };

        let Some(source_id) = world.id_for_path(Path::new(&src_loc.filepath)) else {
            return vec![];
        };
        let Some(source) = world.source(source_id).ok() else {
            return vec![];
        };
        let Some(cursor) = source.lines().line_column_to_byte(line, column) else {
            return vec![];
        };

        jump_from_cursor(doc, &source, cursor)
    }

    fn changed_document_positions(
        &self,
        previous: &dyn tinymist_preview::CompileView,
    ) -> Vec<Position> {
        let Some(previous) = previous.as_any().downcast_ref::<Self>() else {
            return vec![];
        };
        let Some(document) = self.doc() else {
            return vec![];
        };
        let world = self.art.world();
        let previous_world = previous.art.world();
        // Freeze the old dependency set before reading any new source through
        // the old world: a newly included file has no previous compiled text.
        let previous_files: HashSet<_> = previous.art.depended_files().iter().copied().collect();
        let mut edits = SourceEdits::default();

        for file_id in self.art.depended_files().iter().copied() {
            let Ok(source) = world.source(file_id) else {
                continue;
            };
            if !previous_files.contains(&file_id) {
                edits.new_files.insert(file_id);
                continue;
            }
            let Ok(previous_source) = previous_world.source(file_id) else {
                continue;
            };
            let current = source.text();
            let old = previous_source.text();
            if let Some(range) = edited_range(current, old) {
                collect_edit_spans(LinkedNode::new(source.root()), &range, &mut edits.spans);
            }
        }

        let TypstDocument::Paged(paged) = document else {
            return vec![];
        };
        if edits.spans.is_empty() && edits.new_files.is_empty() {
            return vec![];
        }
        let mut positions = Vec::new();
        for (index, page) in paged.pages().iter().enumerate() {
            let mut candidates = PageEditPositions::default();
            find_edit_positions(
                &page.frame,
                Point::zero(),
                &page.frame,
                &edits,
                &mut candidates,
            );
            let page = NonZeroUsize::new(index + 1).unwrap();
            for (rank, point) in [candidates.element, candidates.text, candidates.link]
                .into_iter()
                .enumerate()
            {
                if let Some(point) = point {
                    positions.push((rank, Position { page, point }));
                }
            }
        }
        positions.sort_by_key(|(rank, _)| *rank);
        positions
            .into_iter()
            .map(|(_, position)| position)
            .collect()
    }

    fn resolve_span(&self, span: Span, offset: Option<usize>) -> Option<DocToSrcJumpInfo> {
        let world = self.art.world();
        let resolve_off = |src: &Source, off: usize| {
            src.lines()
                .byte_to_line(off)
                .zip(src.lines().byte_to_column(off))
        };

        let source = world.source(span.id()?).ok()?;
        let mut range = source.find(span)?.range();
        if let Some(off) = offset {
            if off < range.len() {
                range.start += off;
            }
        }

        // todo: resolve untitled uri.
        let filepath = world.path_for_id(span.id()?).ok()?.to_err().ok()?;
        Some(DocToSrcJumpInfo {
            filepath: filepath.to_string_lossy().to_string(),
            start: resolve_off(&source, range.start),
            end: resolve_off(&source, range.end),
        })
    }
}

/// Read exactly one marked occurrence, including generated reference text.
fn marked_highlight_text(
    document: &tinymist_std::typst::TypstPagedDocument,
    token: &str,
) -> Option<String> {
    struct MarkedText<'a> {
        start: &'a str,
        end: &'a str,
        state: u8,
        text: String,
    }
    fn collect(frame: &Frame, marked: &mut MarkedText<'_>) {
        for (_, item) in frame.items() {
            match item {
                FrameItem::Group(group) => collect(&group.frame, marked),
                FrameItem::Tag(Tag::Start(elem, _)) => {
                    if let Some(metadata) = elem.to_packed::<MetadataElem>() {
                        if let typst::foundations::Value::Str(value) = &metadata.value {
                            if value.as_str() == marked.start {
                                marked.state = if marked.state == 0 { 1 } else { 3 };
                            } else if value.as_str() == marked.end {
                                marked.state = if marked.state == 1 { 2 } else { 3 };
                            }
                        }
                    }
                }
                FrameItem::Text(text) if marked.state == 1 => {
                    let mut clusters = HashSet::new();
                    for glyph in &text.glyphs {
                        if clusters.insert(glyph.range()) {
                            if let Some(content) = text.text.get(glyph.range()) {
                                marked.text.push_str(content);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let start = format!("{token}-start");
    let end = format!("{token}-end");
    let mut marked = MarkedText {
        start: &start,
        end: &end,
        state: 0,
        text: String::new(),
    };
    for page in document.pages() {
        collect(&page.frame, &mut marked);
    }
    (marked.state == 2 && !marked.text.is_empty()).then_some(marked.text)
}

#[derive(Default)]
struct SourceEdits {
    spans: HashSet<Span>,
    new_files: HashSet<FileId>,
}

impl SourceEdits {
    fn contains(&self, span: Span) -> bool {
        self.spans.contains(&span) || span.id().is_some_and(|id| self.new_files.contains(&id))
    }
}

/// Bound the edit by its common prefix and suffix. For deletions, map the
/// adjacent surviving token instead of scanning an arbitrary number of bytes.
fn edited_range(current: &str, old: &str) -> Option<Range<usize>> {
    if current == old || current.is_empty() {
        return None;
    }
    let start = current
        .bytes()
        .zip(old.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = current.as_bytes()[start..]
        .iter()
        .rev()
        .zip(old.as_bytes()[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let end = current.len() - suffix;
    Some(if start == end {
        let start = start.min(current.len() - 1);
        start..start + 1
    } else {
        start..end
    })
}

/// Include enclosing equations/figure calls/heading nodes, whose tagged output
/// identifies the actual body location even when headers or outlines copy it.
fn collect_edit_spans(node: LinkedNode<'_>, range: &Range<usize>, spans: &mut HashSet<Span>) {
    let bounds = node.range();
    if bounds.start >= range.end || bounds.end <= range.start {
        return;
    }
    // Markup spans enclose entire files/blocks and would match unrelated text.
    if node.kind() != SyntaxKind::Markup {
        spans.insert(node.span());
    }
    for child in node.children() {
        collect_edit_spans(child, range, spans);
    }
}

#[derive(Default)]
struct PageEditPositions {
    element: Option<Point>,
    text: Option<Point>,
    link: Option<Point>,
}

fn find_edit_positions(
    frame: &Frame,
    offset: Point,
    page: &Frame,
    edits: &SourceEdits,
    positions: &mut PageEditPositions,
) {
    for (pos, item) in frame.items() {
        let mut point = offset + *pos;
        match item {
            FrameItem::Group(group) => {
                // Use the same translated coordinates as source navigation.
                find_edit_positions(&group.frame, point, page, edits, positions);
            }
            FrameItem::Tag(Tag::Start(elem, _))
                if positions.element.is_none()
                    && !elem.is::<OutlineElem>()
                    && !elem.is::<OutlineEntry>()
                    && edits.contains(elem.span()) =>
            {
                positions.element = Some(point);
            }
            FrameItem::Text(text) if positions.text.is_none() => {
                for glyph in &text.glyphs {
                    if edits.contains(glyph.span.0) {
                        if internal_link_at(page, point) {
                            positions.link.get_or_insert(point);
                        } else {
                            positions.text = Some(point);
                            break;
                        }
                    }
                    point.x += glyph.x_advance.at(text.size);
                }
            }
            FrameItem::Shape(_, span) | FrameItem::Image(_, _, span)
                if positions.text.is_none() && edits.contains(*span) =>
            {
                if internal_link_at(page, point) {
                    positions.link.get_or_insert(point);
                } else {
                    positions.text = Some(point);
                }
            }
            _ => {}
        }
    }
}

/// Whether a source jump's text baseline lies inside an internal link.
fn internal_link_at(frame: &Frame, point: Point) -> bool {
    frame.items().any(|(pos, item)| match item {
        FrameItem::Link(Destination::Position(_) | Destination::Location(_), size) => {
            pos.x <= point.x
                && point.x <= pos.x + size.x
                && pos.y <= point.y
                && point.y <= pos.y + size.y
        }
        // Match jump_from_cursor's frame coordinates (translation only).
        FrameItem::Group(group) => internal_link_at(&group.frame, point - *pos),
        _ => false,
    })
}

/// Gather one entry per shaped text cluster, preserving repeated source uses.
fn collect_source_glyphs(
    frame: &Frame,
    source: &Source,
    output: &mut Vec<super::highlight::RenderedGlyph>,
) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(group) => collect_source_glyphs(&group.frame, source, output),
            FrameItem::Text(text) => {
                let mut clusters = HashSet::new();
                for glyph in &text.glyphs {
                    if glyph.span.0.id() != Some(source.id()) || !clusters.insert(glyph.range()) {
                        continue;
                    }
                    let Some(node) = source.find(glyph.span.0) else {
                        continue;
                    };
                    let Some(content) = text.text.get(glyph.range()) else {
                        continue;
                    };
                    let cluster_offset = if node.kind() == SyntaxKind::MathText {
                        usize::from(glyph.span.1).max(glyph.range().start)
                    } else {
                        usize::from(glyph.span.1)
                    };
                    let offset = node.range().start + cluster_offset;
                    output.push(super::highlight::RenderedGlyph {
                        offset,
                        end: if node.kind() == SyntaxKind::Text {
                            offset + content.len()
                        } else {
                            node.range().end
                        },
                        text: content.to_owned(),
                    });
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use typst::layout::{FrameKind, GroupItem, Size};
    use typst::model::Url;

    use super::*;

    fn pt(x: f64, y: f64) -> Point {
        Point::new(Abs::pt(x), Abs::pt(y))
    }

    #[test]
    fn edit_range_handles_insertions_deletions_and_unicode() {
        assert_eq!(edited_range("abXYcd", "abcd"), Some(2..4));
        assert_eq!(edited_range("abcd", "abXYcd"), Some(2..3));
        assert_eq!(edited_range("ab", "abcd"), Some(1..2));
        assert_eq!(edited_range("", "abcd"), None);
        assert_eq!(edited_range("same", "same"), None);
        let current = "수식 $beta$ 끝";
        let old = "수식 $alpha$ 끝";
        let range = edited_range(current, old).unwrap();
        let source = Source::detached(current);
        let mut spans = HashSet::new();
        collect_edit_spans(LinkedNode::new(source.root()), &range, &mut spans);
        let cursor = current.find("beta").unwrap() + 1;
        let node = LinkedNode::new(source.root())
            .leaf_at_compat(cursor)
            .unwrap();
        assert_eq!(node.kind(), SyntaxKind::MathIdent);
        assert!(spans.contains(&node.span()));
        assert!(!spans.contains(&source.root().span()));
    }

    #[test]
    fn internal_link_in_nested_frame_includes_baseline_edges() {
        let mut child = Frame::new(Size::new(Abs::pt(100.), Abs::pt(100.)), FrameKind::Soft);
        child.push(
            pt(5., 10.),
            FrameItem::Link(
                Destination::Position(Position {
                    page: NonZeroUsize::new(120).unwrap(),
                    point: pt(24., 40.),
                }),
                Size::new(Abs::pt(50.), Abs::pt(12.)),
            ),
        );
        let mut page = Frame::new(child.size(), FrameKind::Hard);
        page.push(pt(20., 30.), FrameItem::Group(GroupItem::new(child)));
        assert!(internal_link_at(&page, pt(25., 52.)));
        assert!(internal_link_at(&page, pt(75., 52.)));
        assert!(!internal_link_at(&page, pt(25., 53.)));
        assert!(!internal_link_at(&page, pt(24., 52.)));
    }

    #[test]
    fn external_links_do_not_displace_authored_text() {
        let mut frame = Frame::new(Size::new(Abs::pt(100.), Abs::pt(100.)), FrameKind::Soft);
        frame.push(
            Point::zero(),
            FrameItem::Link(
                Destination::Url(Url::new("https://typst.app").unwrap()),
                frame.size(),
            ),
        );
        assert!(!internal_link_at(&frame, pt(10., 10.)));
    }
}
