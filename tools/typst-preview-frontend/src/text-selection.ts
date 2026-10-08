export interface DocumentPoint {
  page_no: number;
  x: number;
  y: number;
}

export interface TextFragment {
  page: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface PageGeometry {
  element: Element;
  rect: DOMRect;
  width: number;
  height: number;
  fingerprint: string;
}

export interface SelectedText {
  revision?: string;
  text: string;
  start: DocumentPoint;
  end: DocumentPoint;
  fragments: TextFragment[];
  fingerprints: Record<number, string>;
}

/** Page units stay independent of renderer scale and native pinch zoom. */
export function pageFragment(
  page: number,
  rect: Pick<DOMRect, "left" | "top" | "width" | "height">,
  geometry: Pick<PageGeometry, "rect" | "width" | "height">,
): TextFragment {
  return {
    page,
    x: ((rect.left - geometry.rect.left) * geometry.width) / geometry.rect.width,
    y: ((rect.top - geometry.rect.top) * geometry.height) / geometry.rect.height,
    width: (rect.width * geometry.width) / geometry.rect.width,
    height: (rect.height * geometry.height) / geometry.rect.height,
  };
}

function fingerprint(value: string) {
  let hash = 2166136261;
  for (let i = 0; i < value.length; i++) hash = Math.imul(hash ^ value.charCodeAt(i), 16777619);
  return (hash >>> 0).toString(16);
}

export function previewPages(root: HTMLElement): Map<number, PageGeometry> {
  const pages = new Map<number, PageGeometry>();
  for (const element of root.querySelectorAll(".typst-page-inner")) {
    const page = Number(element.getAttribute("data-page-number")) + 1;
    const group = root.querySelector(`.typst-page[data-page-number="${page - 1}"]`);
    const lines = Array.from(group?.querySelectorAll(".tsel") || []);
    if (!lines.length) continue;
    const width = Number(element.getAttribute("data-page-width"));
    const height = Number(element.getAttribute("data-page-height"));
    const rect = element.getBoundingClientRect();
    if (!width || !height || !rect.width || !rect.height) continue;
    const geometry = { element, rect, width, height, fingerprint: "" };
    if (lines.length) {
      // Text plus layout: changing a preceding figure must invalidate old
      // coordinates even if the page still contains the same words.
      geometry.fingerprint = fingerprint(
        JSON.stringify([
          width,
          height,
          ...lines.map((line) => {
            const bounds = pageFragment(
              page,
              line.parentElement!.getBoundingClientRect(),
              geometry,
            );
            return [
              line.textContent,
              ...[bounds.x, bounds.y, bounds.width, bounds.height].map(
                (v) => Math.round(v * 2) / 2,
              ),
            ];
          }),
        ]),
      );
    }
    pages.set(page, geometry);
  }
  return pages;
}

/** Capture only the rendered text layer, never toolbar or unrelated DOM text. */
export function selectedPreviewText(root: HTMLElement): SelectedText | undefined {
  const selection = root.ownerDocument.getSelection();
  if (!selection || selection.isCollapsed || !selection.rangeCount) return;
  const range = selection.getRangeAt(0);
  const surface = root.querySelector<HTMLElement>(":scope > .typst-touch-selection") || root;
  const belongs = (node: Node) =>
    surface.contains(node) &&
    (surface !== root ||
      !!(node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement)?.closest(
        ".tsel",
      ));
  if (!belongs(range.startContainer) || !belongs(range.endContainer)) return;
  const pages = previewPages(root);
  const fragments: TextFragment[] = [];
  const fingerprints: Record<number, string> = {};
  let text = "";
  let start: DocumentPoint | undefined;
  let end: DocumentPoint | undefined;
  let previous: TextFragment | undefined;
  const selector = surface === root ? ".tsel" : ".typst-touch-selection-line";
  for (const line of surface.querySelectorAll<HTMLElement>(selector)) {
    if (!range.intersectsNode(line)) continue;
    const page =
      Number(line.dataset.page) ||
      Number(line.closest(".typst-page")?.getAttribute("data-page-number")) + 1;
    const geometry = pages.get(page);
    if (!geometry?.fingerprint) continue;
    const walker = root.ownerDocument.createTreeWalker(line, NodeFilter.SHOW_TEXT);
    let node: Node | null;
    while ((node = walker.nextNode())) {
      if (!range.intersectsNode(node) || !node.textContent) continue;
      const piece = root.ownerDocument.createRange();
      piece.selectNodeContents(node);
      if (range.compareBoundaryPoints(Range.START_TO_START, piece) > 0)
        piece.setStart(range.startContainer, range.startOffset);
      if (range.compareBoundaryPoints(Range.END_TO_END, piece) < 0)
        piece.setEnd(range.endContainer, range.endOffset);
      const quote = piece.toString();
      if (!quote) continue;
      const rects = Array.from(piece.getClientRects()).filter(
        (rect) => rect.width > 0 && rect.height > 0,
      );
      if (!rects.length) continue;
      const part = pageFragment(page, rects[0], geometry);
      if (previous && (previous.page !== page || Math.abs(previous.y - part.y) > part.height * 0.6))
        text += "\n";
      text += quote;
      for (const rect of rects) fragments.push(pageFragment(page, rect, geometry));
      fingerprints[page] = geometry.fingerprint;
      // Hit-test glyph centers instead of the boundary between adjacent glyphs.
      const glyph = piece.cloneRange();
      glyph.setEnd(piece.startContainer, piece.startOffset + Array.from(quote)[0].length);
      const first = pageFragment(page, glyph.getBoundingClientRect(), geometry);
      glyph.setStart(piece.endContainer, piece.endOffset - Array.from(quote).at(-1)!.length);
      glyph.setEnd(piece.endContainer, piece.endOffset);
      const last = pageFragment(page, glyph.getBoundingClientRect(), geometry);
      start ??= { page_no: page, x: first.x + first.width * 0.25, y: first.y + first.height * 0.5 };
      end = { page_no: page, x: last.x + last.width * 0.25, y: last.y + last.height * 0.5 };
      previous = part;
    }
  }
  if (!start || !end || !text.trim() || Array.from(text).length > 8000 || fragments.length > 256)
    return;
  return { text, start, end, fragments, fingerprints };
}
