/** Intersection in layout-viewport CSS pixels, including native pinch panning. */
export function visibleVerticalBounds(
  scroller: { top: number; bottom: number },
  viewport?: { offsetTop: number; height: number } | null,
) {
  if (!viewport) return scroller;
  const top = Math.max(scroller.top, viewport.offsetTop);
  const bottom = Math.min(scroller.bottom, viewport.offsetTop + viewport.height);
  return bottom > top ? { top, bottom } : scroller;
}
