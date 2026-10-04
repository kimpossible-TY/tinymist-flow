/** Place feedback inside the visible safe area without changing document zoom. */
export function focusStatusPlacement(
  viewport: Pick<VisualViewport, "offsetLeft" | "offsetTop" | "width" | "height" | "scale">,
  insets: { left: number; right: number; bottom: number },
) {
  const scale = viewport.scale || 1;
  const left = Math.max(12, insets.left) / scale;
  const right = Math.max(12, insets.right) / scale;
  const bottom = Math.max(12, insets.bottom) / scale;
  return {
    left: `${viewport.offsetLeft + (viewport.width + left - right) / 2}px`,
    top: `${viewport.offsetTop + viewport.height - bottom}px`,
    right: "auto",
    bottom: "auto",
    margin: "0",
    maxWidth: `${Math.max(1, (viewport.width - left - right) * scale)}px`,
    transform: `translate(-50%, -100%) scale(${1 / scale})`,
    transformOrigin: "bottom center",
  };
}
