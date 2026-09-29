/** Only a real range in the mobile HTML overlay holds rendering. */
export function hasTouchSelection(root: Element, selection = root.ownerDocument.getSelection()) {
  if (!selection || selection.isCollapsed) return false;
  const overlay = root.querySelector(":scope > .typst-touch-selection");
  return (
    !!overlay && (overlay.contains(selection.anchorNode) || overlay.contains(selection.focusNode))
  );
}
