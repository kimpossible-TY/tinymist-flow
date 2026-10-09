//! Conservative source edits for explicit text ranges in preview.

use std::collections::HashSet;
use std::io::Write;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use tinymist_preview::PreviewHighlightStatus;
use typst::syntax::{ast, LinkedNode, Side, Source, SyntaxKind};

/// Use the document's helper when it provides math-aware highlighting.
#[derive(Clone, Copy)]
pub(super) enum HighlightFunction {
    Standard,
    MathAware,
}

/// A rendered text cluster and its origin in this source file.
pub(super) struct RenderedGlyph {
    pub offset: usize,
    pub end: usize,
    pub text: String,
}

/// Verify rendered coverage before editing content, including complete equations.
/// A repeated source origin cannot identify which visible occurrence to edit.
pub(super) fn selection_range(
    source: &Source,
    first: usize,
    last: usize,
    text: &str,
    glyphs: &[RenderedGlyph],
    function: HighlightFunction,
) -> Option<Range<usize>> {
    let (first, last) = (first.min(last), first.max(last));
    if last >= source.text().len() || last - first > 64 * 1024 {
        return None;
    }
    if let Some(range) = literal_range(source, first, last, text) {
        return coverage_matches(&range, text, glyphs, false).then_some(range);
    }
    if !matches!(function, HighlightFunction::MathAware) {
        return None;
    }
    let root = LinkedNode::new(source.root());
    let first_node = root.leaf_at(first, Side::After)?;
    if !editable_context(&first_node) {
        return None;
    }
    let last_node = root.leaf_at(last, Side::After)?;
    let mut starts = if let Some(equation) = equation_range(first_node.clone()) {
        vec![equation.start]
    } else {
        // Source jumps describe a caret, which can be on either side of the
        // clicked glyph's center. Verify both adjacent character boundaries.
        let mut starts = vec![first];
        if let Some(previous) = source
            .text()
            .char_indices()
            .take_while(|(i, _)| *i < first)
            .last()
            .map(|(i, _)| i)
            .filter(|i| *i >= first_node.offset())
        {
            starts.push(previous);
        }
        starts
    };
    // A point between glyphs can resolve to an enclosing content call rather
    // than the literal child. Include the nearest rendered origins around that
    // bounded source hint; syntax and full coverage still require one match.
    let mut nearby: Vec<_> = glyphs
        .iter()
        .map(|glyph| glyph.offset)
        .filter(|offset| offset.abs_diff(first) <= 128)
        .collect();
    nearby.sort_unstable();
    nearby.dedup();
    let neighbors = nearby
        .iter()
        .copied()
        .rfind(|offset| *offset <= first)
        .into_iter()
        .chain(
            nearby
                .iter()
                .copied()
                .filter(|offset| *offset > first)
                .take(2),
        );
    for offset in neighbors {
        let node = root.leaf_at(offset, Side::After)?;
        starts.push(equation_range(node).map_or(offset, |equation| equation.start));
    }
    starts.sort_unstable();
    starts.dedup();
    let ends = if let Some(equation) = equation_range(last_node) {
        vec![equation.end]
    } else {
        let end = glyphs
            .iter()
            .find(|glyph| glyph.offset == last)
            .map(|glyph| last + glyph.text.len())
            .or_else(|| {
                source
                    .text()
                    .char_indices()
                    .find(|(i, _)| *i > last)
                    .map(|(i, _)| i)
            })
            .unwrap_or(source.text().len());
        vec![last, end]
    };
    let mut matches = vec![];
    for start in starts {
        for &end in &ends {
            if start >= end
                || end > source.text().len()
                || !source.text().is_char_boundary(start)
                || !source.text().is_char_boundary(end)
            {
                continue;
            }
            let candidate = &source.text()[start..end];
            let range = (start + candidate.len() - candidate.trim_start().len())
                ..(end - candidate.len() + candidate.trim_end().len());
            if root
                .leaf_at(range.start, Side::After)
                .is_some_and(|node| editable_context(&node))
                && reviewable_markup(root.clone(), &range, true, false)
                && contains_equation(root.clone(), &range)
                && coverage_matches(&range, text, glyphs, true)
            {
                matches.push(range);
            }
        }
    }
    matches.sort_unstable_by_key(|range| (range.start, range.end));
    matches.dedup();
    (matches.len() == 1).then(|| matches.remove(0))
}

/// Reference output can originate in a package rather than at its call site.
/// These syntax-bounded candidates require a separate marked compiler render;
/// source spelling or a reference's shared glyph origin cannot verify them.
pub(super) fn reference_ranges(
    source: &Source,
    first: usize,
    last: usize,
    glyphs: &[RenderedGlyph],
    function: HighlightFunction,
) -> Vec<Range<usize>> {
    let (first, last) = (first.min(last), first.max(last));
    if last >= source.text().len()
        || last - first > 64 * 1024
        || !source.text().is_char_boundary(first)
        || !source.text().is_char_boundary(last)
    {
        return vec![];
    }
    let root = LinkedNode::new(source.root());
    let Some(first_node) = root.leaf_at(first, Side::After) else {
        return vec![];
    };
    let Some(last_node) = root.leaf_at(last, Side::After) else {
        return vec![];
    };
    if !editable_context(&first_node) {
        return vec![];
    }
    let starts = if let Some(equation) = equation_range(first_node.clone()) {
        vec![equation.start]
    } else {
        let mut starts = vec![first];
        if let Some(previous) = source.text()[..first].char_indices().last() {
            if previous.0 >= first_node.offset() {
                starts.push(previous.0);
            }
        }
        starts
    };
    let ends = if let Some(equation) = equation_range(last_node) {
        vec![equation.end]
    } else {
        vec![
            last,
            glyphs
                .iter()
                .find(|glyph| glyph.offset == last)
                .map(|glyph| glyph.end)
                .unwrap_or_else(|| last + source.text()[last..].chars().next().unwrap().len_utf8()),
        ]
    };
    let math = matches!(function, HighlightFunction::MathAware);
    let mut candidates = vec![];
    for start in starts {
        for &end in &ends {
            if start >= end
                || end > source.text().len()
                || !source.text().is_char_boundary(start)
                || !source.text().is_char_boundary(end)
            {
                continue;
            }
            let candidate = &source.text()[start..end];
            let range = (start + candidate.len() - candidate.trim_start().len())
                ..(end - candidate.len() + candidate.trim_end().len());
            let mut origins = HashSet::new();
            let repeated = glyphs
                .iter()
                .filter(|glyph| range.contains(&glyph.offset))
                .any(|glyph| {
                    // A reference's generated glyphs may all share its call span.
                    // Literal text and equation origins still must be single-use.
                    root.leaf_at(glyph.offset, Side::After).is_some_and(|node| {
                        equation_range(node.clone()).is_some() || node.kind() == SyntaxKind::Text
                    }) && !origins.insert(glyph.offset)
                });
            if !repeated
                && contains_reference(root.clone(), &range)
                && reviewable_markup(root.clone(), &range, math, true)
            {
                candidates.push(range);
            }
        }
    }
    candidates.sort_unstable_by_key(|range| (range.start, range.end));
    candidates.dedup();
    candidates
}

fn reference_call(node: &LinkedNode<'_>) -> bool {
    let Some(call) = node.get().cast::<ast::FuncCall>() else {
        return false;
    };
    let mut callee = call.callee();
    while let ast::Expr::Parenthesized(expr) = callee {
        callee = expr.expr();
    }
    match callee {
        ast::Expr::Ident(ident) => ident.as_str() == "ref",
        ast::Expr::FieldAccess(access) => access.field().as_str() == "ref",
        _ => false,
    }
}

fn contains_reference(node: LinkedNode<'_>, range: &Range<usize>) -> bool {
    if node.range().end <= range.start || node.offset() >= range.end {
        return false;
    }
    node.kind() == SyntaxKind::Ref
        || reference_call(&node)
        || node
            .children()
            .any(|child| contains_reference(child, range))
}

/// Compare complete rendered coverage, allowing mathematical layout order.
pub(super) fn rendered_reference_matches(rendered: &str, selected: &str, math: bool) -> bool {
    if math {
        characters(rendered) == characters(selected)
    } else {
        normalize(rendered).0.trim() == normalize(selected).0.trim()
    }
}

fn coverage_matches(
    range: &Range<usize>,
    text: &str,
    glyphs: &[RenderedGlyph],
    math: bool,
) -> bool {
    let mut origins = HashSet::new();
    let mut rendered = String::new();
    for glyph in glyphs.iter().filter(|glyph| range.contains(&glyph.offset)) {
        if glyph.end > range.end || !origins.insert(glyph.offset) {
            return false;
        }
        rendered.push_str(&glyph.text);
    }
    if origins.is_empty() {
        return false;
    }
    // Math layout can reorder fractions and scripts relative to source order.
    // Endpoints and syntax constrain the source interval; all visible characters
    // must still be covered, irrespective of that layout ordering.
    if math {
        characters(&rendered) == characters(text)
    } else {
        normalize(&rendered).0.trim() == normalize(text).0.trim()
    }
}

fn characters(text: &str) -> Vec<char> {
    let mut chars: Vec<_> = text.chars().filter(|c| !c.is_whitespace()).collect();
    chars.sort_unstable();
    chars
}

fn equation_range(mut node: LinkedNode<'_>) -> Option<Range<usize>> {
    loop {
        if node.kind() == SyntaxKind::Equation {
            return Some(node.range());
        }
        node = node.parent()?.clone();
    }
}

fn editable_context(node: &LinkedNode<'_>) -> bool {
    let mut parent = node.clone();
    loop {
        if matches!(
            parent.kind(),
            SyntaxKind::LetBinding
                | SyntaxKind::ShowRule
                | SyntaxKind::SetRule
                | SyntaxKind::CodeBlock
                | SyntaxKind::ForLoop
                | SyntaxKind::WhileLoop
        ) {
            return false;
        }
        let Some(next) = parent.parent() else {
            return true;
        };
        parent = next.clone();
    }
}

fn contains_equation(node: LinkedNode<'_>, range: &Range<usize>) -> bool {
    if node.range().end <= range.start || node.offset() >= range.end {
        return false;
    }
    node.kind() == SyntaxKind::Equation
        || node.children().any(|child| contains_equation(child, range))
}

/// Find one literal source range containing both mapped glyphs. Whitespace
/// normalization accounts for visual line wrapping without guessing markup.
pub(super) fn literal_range(
    source: &Source,
    first: usize,
    last: usize,
    text: &str,
) -> Option<Range<usize>> {
    if first > last || last >= source.text().len() || last - first > 64 * 1024 {
        return None;
    }
    let mut lo = first.saturating_sub(128);
    let mut hi = (last + text.len() + 128).min(source.text().len());
    while !source.text().is_char_boundary(lo) {
        lo -= 1;
    }
    while !source.text().is_char_boundary(hi) {
        hi += 1;
    }
    let (haystack, offsets) = normalize(&source.text()[lo..hi]);
    let needle = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if needle.is_empty() {
        return None;
    }
    if !editable_context(&LinkedNode::new(source.root()).leaf_at(first, Side::After)?) {
        return None;
    }
    let mut matches = haystack
        .char_indices()
        .take_while(|(index, _)| lo + offsets[*index].0 <= first)
        .filter_map(|(index, _)| {
            if !haystack[index..].starts_with(&needle) {
                return None;
            }
            let range = (lo + offsets[index].0)..(lo + offsets[index + needle.len() - 1].1);
            (range.contains(&first)
                && range.contains(&last)
                && reviewable_markup(LinkedNode::new(source.root()), &range, false, false))
            .then_some(range)
        });
    let range = matches.next()?;
    matches.next().is_none().then_some(range)
}

fn normalize(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut result = String::new();
    let mut offsets: Vec<(usize, usize)> = Vec::new();
    for (index, character) in text.char_indices() {
        let end = index + character.len_utf8();
        if character.is_whitespace() && result.ends_with(' ') {
            offsets.last_mut().unwrap().1 = end;
            continue;
        }
        let normalized = if character.is_whitespace() {
            ' '
        } else {
            character
        };
        result.push(normalized);
        offsets.extend(std::iter::repeat_n((index, end), normalized.len_utf8()));
    }
    (result, offsets)
}

fn reviewable_markup(
    node: LinkedNode<'_>,
    range: &Range<usize>,
    math: bool,
    references: bool,
) -> bool {
    if node.range().end <= range.start || node.offset() >= range.end {
        return true;
    }
    if math && node.kind() == SyntaxKind::Equation {
        return range.start <= node.offset() && range.end >= node.range().end;
    }
    if references && (node.kind() == SyntaxKind::Ref || reference_call(&node)) {
        return range.start <= node.offset() && range.end >= node.range().end;
    }
    if node.children().next().is_none() {
        if references && node.kind() == SyntaxKind::Hash {
            return node
                .next_sibling()
                .is_some_and(|next| reference_call(&next) && range.end >= next.range().end);
        }
        return matches!(node.kind(), SyntaxKind::Text | SyntaxKind::Space);
    }
    node.children()
        .all(|child| reviewable_markup(child, range, math, references))
}

/// Construct the candidate source for parse and compiler validation.
pub(super) fn highlighted_source(
    source: &Source,
    range: Range<usize>,
    function: HighlightFunction,
) -> String {
    let mut updated = source.text().to_owned();
    updated.insert(range.end, ']');
    updated.insert_str(
        range.start,
        match function {
            HighlightFunction::Standard => "#highlight[",
            HighlightFunction::MathAware => "#highlighted[",
        },
    );
    updated
}

/// Delimit the candidate output with invisible, compiler-only metadata.
pub(super) fn marked_highlight_source(
    source: &Source,
    range: Range<usize>,
    function: HighlightFunction,
    token: &str,
) -> String {
    let mut updated = highlighted_source(source, range.clone(), function);
    let added = updated.len() - source.text().len();
    updated.insert_str(range.end + added, &format!("#metadata(\"{token}-end\")"));
    updated.insert_str(range.start, &format!("#metadata(\"{token}-start\")"));
    updated
}

/// Replace a verified disk snapshot, keeping file permissions and rejecting
/// syntax errors. Serialize edits from independent light/dark preview views.
pub(super) fn write_highlight(
    path: &Path,
    source: &Source,
    range: Range<usize>,
    function: HighlightFunction,
) -> PreviewHighlightStatus {
    static EDIT_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let _lock = EDIT_LOCK.lock();
    let Ok(disk) = std::fs::read_to_string(path) else {
        return PreviewHighlightStatus::Error;
    };
    if disk != source.text() {
        return PreviewHighlightStatus::Stale;
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return PreviewHighlightStatus::Error;
    };
    if metadata.permissions().readonly() {
        return PreviewHighlightStatus::Unsupported;
    }
    let updated = highlighted_source(source, range, function);
    if Source::detached(updated.clone()).root().diagnosis().errors {
        return PreviewHighlightStatus::Unmapped;
    }
    let temp = path.with_extension(format!(
        "flow-highlight-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> std::io::Result<PreviewHighlightStatus> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.set_permissions(metadata.permissions())?;
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
        // Recheck immediately before replacement; never overwrite a known
        // editor change that arrived while the temporary file was written.
        if std::fs::read_to_string(path)? != disk {
            return Ok(PreviewHighlightStatus::Stale);
        }
        std::fs::rename(&temp, path)?;
        Ok(PreviewHighlightStatus::Highlighted)
    })();
    let _ = std::fs::remove_file(temp);
    match result {
        Ok(status) => status,
        Err(error) => {
            log::warn!("Could not write preview highlight: {error}");
            PreviewHighlightStatus::Error
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_unicode_partial_words_and_visual_line_wraps() {
        let source = Source::detached("앞 문장. The convergent power\n  series ends here.");
        let first = source.text().find("convergent").unwrap();
        let last = source.text().find("series").unwrap() + 5;
        let range = literal_range(&source, first, last, "convergent power\nseries").unwrap();
        assert_eq!(&source.text()[range], "convergent power\n  series");
        assert_eq!(literal_range(&source, 4, 7, "문장"), Some(4..10));
        let first = source.text().find("convergent").unwrap() + 3;
        assert_eq!(
            literal_range(&source, first, first + 3, "vergent"),
            Some(first..first + 7)
        );
    }

    #[test]
    fn endpoints_disambiguate_repeated_text_and_reject_generated_or_markup_ranges() {
        let source = Source::detached("Repeat text. Repeat text.");
        assert_eq!(literal_range(&source, 13, 22, "Repeat text"), Some(13..24));
        assert!(literal_range(&source, 0, 22, "Repeat text").is_none());
        assert!(literal_range(&Source::detached("aaaa"), 1, 1, "aa").is_none());
        for text in [
            "#let a = \"Hello world\"",
            "$ Hello world $",
            "Hello *world*",
            "Hello #text[world]",
            "#let a = [Hello world]",
            "#for i in range(3) [Hello world]",
        ] {
            let source = Source::detached(text);
            let start = text.find("Hello").unwrap();
            let end = text.find("world").unwrap() + 4;
            assert!(
                literal_range(&source, start, end, "Hello world").is_none(),
                "{text}"
            );
        }
    }

    #[test]
    fn writes_real_highlight_and_rejects_changed_disk() {
        let path =
            std::env::temp_dir().join(format!("flow-highlight-test-{}.typ", std::process::id()));
        let source = Source::detached("Before 선택한 text after.");
        std::fs::write(&path, source.text()).unwrap();
        let start = source.text().find("선택한").unwrap();
        let range = start..start + "선택한 text".len();
        assert!(matches!(
            write_highlight(&path, &source, range.clone(), HighlightFunction::Standard),
            PreviewHighlightStatus::Highlighted
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "Before #highlight[선택한 text] after."
        );
        assert!(matches!(
            write_highlight(&path, &source, range, HighlightFunction::Standard),
            PreviewHighlightStatus::Stale
        ));
        std::fs::remove_file(path).unwrap();
    }

    fn make_glyphs(source: &str, clusters: &[(&str, &str)]) -> Vec<RenderedGlyph> {
        clusters
            .iter()
            .map(|(origin, text)| {
                let offset = source.find(origin).unwrap();
                let parsed = Source::detached(source);
                let root = LinkedNode::new(parsed.root());
                let node = root.leaf_at(offset, Side::After).unwrap();
                RenderedGlyph {
                    offset,
                    end: if equation_range(node.clone()).is_some() {
                        node.range().end
                    } else {
                        offset + text.len()
                    },
                    text: (*text).into(),
                }
            })
            .collect()
    }

    #[test]
    fn math_uses_complete_rendered_coverage_instead_of_source_spelling() {
        let source = Source::detached("Before $alpha^2$ after.");
        let glyphs = make_glyphs(
            source.text(),
            &[
                ("Before", "Before "),
                ("alpha", "α"),
                ("2", "2"),
                ("after", "after."),
            ],
        );
        let first = source.text().find("alpha").unwrap();
        let last = source.text().find('2').unwrap();
        assert_eq!(
            selection_range(
                &source,
                first,
                last,
                "2α",
                &glyphs,
                HighlightFunction::MathAware
            ),
            Some(7..16)
        );
        assert!(selection_range(
            &source,
            first,
            first,
            "α",
            &glyphs,
            HighlightFunction::MathAware
        )
        .is_none());
        assert!(selection_range(
            &source,
            first,
            last,
            "α2",
            &glyphs,
            HighlightFunction::Standard
        )
        .is_none());
        assert_eq!(
            selection_range(
                &source,
                0,
                17,
                "Before 2α after.",
                &glyphs,
                HighlightFunction::MathAware
            ),
            Some(0..source.text().len())
        );
        // Click-to-source jumps can land just after the first selected glyph.
        assert_eq!(
            selection_range(
                &source,
                1,
                source.text().len() - 1,
                "Before 2α after.",
                &glyphs,
                HighlightFunction::MathAware
            ),
            Some(0..source.text().len())
        );
    }

    #[test]
    fn single_use_closure_content_is_editable_but_repeated_origins_are_not() {
        let source = Source::detached("#scope(s => ([Hello world]))");
        let first = source.text().find("Hello").unwrap();
        let glyphs = make_glyphs(source.text(), &[("Hello", "Hello world")]);
        assert_eq!(
            selection_range(
                &source,
                first,
                first,
                "Hello world",
                &glyphs,
                HighlightFunction::Standard
            ),
            Some(first..first + 11)
        );
        let repeated = make_glyphs(
            source.text(),
            &[("Hello", "Hello world"), ("Hello", "Hello world")],
        );
        assert!(selection_range(
            &source,
            first,
            first,
            "Hello world",
            &repeated,
            HighlightFunction::Standard
        )
        .is_none());
    }

    #[test]
    fn mixed_math_can_resolve_an_endpoint_on_its_enclosing_content_call() {
        let source = Source::detached("#scope(s => ([Mixed formula $alpha + x$ ends here.]))");
        let glyphs = make_glyphs(
            source.text(),
            &[
                ("Mixed", "Mixed formula "),
                ("alpha", "𝛼"),
                ("+", "+"),
                ("x$", "𝑥"),
                ("ends", "ends here."),
            ],
        );
        let start = source.text().find("Mixed").unwrap();
        let end = source.text().find("here.").unwrap() + 5;
        assert_eq!(
            selection_range(
                &source,
                0,
                end - 1,
                "Mixed formula 𝛼+𝑥 ends here.",
                &glyphs,
                HighlightFunction::MathAware
            ),
            Some(start..end)
        );
    }

    #[test]
    fn writes_the_documents_math_aware_helper() {
        let path = std::env::temp_dir().join(format!(
            "flow-math-highlight-test-{}.typ",
            std::process::id()
        ));
        let source = Source::detached("Text $alpha^2$ end.");
        std::fs::write(&path, source.text()).unwrap();
        assert!(matches!(
            write_highlight(&path, &source, 0..14, HighlightFunction::MathAware),
            PreviewHighlightStatus::Highlighted
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "#highlighted[Text $alpha^2$] end."
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reference_candidates_require_whole_calls_and_compiler_coverage() {
        let source = Source::detached("Before $v$ in #(s.ref)(\"eq\") after.");
        let glyphs = make_glyphs(
            source.text(),
            &[("Before", "Before "), ("v$", "𝑣"), ("after", "after.")],
        );
        let end = source.text().len() - 1;
        assert!(selection_range(
            &source,
            0,
            end,
            "Before 𝑣 in Equation (1) after.",
            &glyphs,
            HighlightFunction::MathAware
        )
        .is_none());
        assert_eq!(
            reference_ranges(&source, 0, end, &glyphs, HighlightFunction::MathAware),
            vec![0..end, 0..end + 1]
        );
        assert!(reference_ranges(
            &source,
            0,
            source.text().find("eq").unwrap(),
            &glyphs,
            HighlightFunction::MathAware
        )
        .is_empty());
        assert!(reference_ranges(&source, 0, end, &glyphs, HighlightFunction::Standard).is_empty());
        let unknown = Source::detached("Before #text[other] after.");
        assert!(reference_ranges(
            &unknown,
            0,
            unknown.text().len() - 1,
            &[],
            HighlightFunction::MathAware
        )
        .is_empty());
        let repeated = make_glyphs(
            source.text(),
            &[("Before", "Before "), ("Before", "Before ")],
        );
        assert!(
            reference_ranges(&source, 0, end, &repeated, HighlightFunction::MathAware).is_empty()
        );
    }
}
