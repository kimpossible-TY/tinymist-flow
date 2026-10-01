import { afterEach, describe, expect, it, vi } from "vitest";
import { hasTouchSelection } from "./touch-selection.mjs";
import { visibleVerticalBounds } from "./visual-viewport.mjs";
import { TypstDocumentContext } from "./typst-doc.mjs";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function fixture() {
  const selected = {} as Node;
  const selection = {
    isCollapsed: false,
    anchorNode: selected as Node | null,
    focusNode: selected as Node | null,
  };
  const ownerDocument = {
    getSelection: () => selection,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  };
  const root = {
    ownerDocument,
    classList: { add: vi.fn() },
    querySelector: () => ({ contains: (node: Node) => node === selected }),
  } as unknown as HTMLElement;
  return { root, selection, ownerDocument };
}

describe("mobile selection render lifetime", () => {
  it("holds ranges in either end of the overlay, not carets or unrelated ranges", () => {
    const { root, selection } = fixture();
    expect(hasTouchSelection(root)).toBe(true);
    selection.isCollapsed = true;
    expect(hasTouchSelection(root)).toBe(false);
    selection.isCollapsed = false;
    selection.anchorNode = null;
    expect(hasTouchSelection(root)).toBe(true);
    selection.focusNode = null;
    expect(hasTouchSelection(root)).toBe(false);
    selection.isCollapsed = true;
    expect(hasTouchSelection(root)).toBe(false);
    expect(hasTouchSelection(root, null)).toBe(false);
  });

  it("retains queued updates while selected, resumes in order, and removes the listener", async () => {
    const { root, selection, ownerDocument } = fixture();
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal("document", { documentElement: {} });
    vi.stubGlobal("getComputedStyle", () => ({ getPropertyValue: () => "white" }));
    vi.spyOn(TypstDocumentContext.prototype as any, "installRescaleHandler").mockImplementation(
      () => {},
    );
    const ctx = new TypstDocumentContext({
      hookedElem: root,
      windowElem: {},
      kModule: {},
      retrieveDOMState: () => ({ width: 400, height: 800, boundingRect: { top: 0, left: 0 } }),
    } as any);
    const process = vi.spyOn(ctx as any, "processQueue").mockReturnValue(true);
    const render = vi.fn();
    (ctx as any).r = { rescale: vi.fn(), rerender: render, postRender: vi.fn() };
    ctx.addChangement(["diff-v1", "first"]);
    ctx.addChangement(["diff-v1", "second"]);
    await frames.shift()!(0);
    ctx.addViewportChange();
    await frames.shift()!(0);
    expect(process).not.toHaveBeenCalled();
    expect(render).not.toHaveBeenCalled();
    expect(ctx.patchQueue).toEqual([
      ["diff-v1", "first"],
      ["diff-v1", "second"],
      ["viewport-change", ""],
    ]);
    selection.isCollapsed = true;
    const onSelection = ownerDocument.addEventListener.mock.calls[0][1];
    onSelection();
    await frames.shift()!(0);
    expect(process.mock.calls.map(([change]) => change)).toEqual([
      ["diff-v1", "first"],
      ["diff-v1", "second"],
      ["viewport-change", ""],
    ]);
    expect(render).toHaveBeenCalledOnce();
    await frames.shift()!(0);
    ctx.dispose();
    expect(ownerDocument.removeEventListener).toHaveBeenCalledWith("selectionchange", onSelection);
    // A theme transition may dispose after a frame is queued but before it runs.
    ctx.addChangement(["diff-v1", "after disposal"]);
    expect(frames).toHaveLength(0);
  });

  it("does not render a queued animation frame after disposal", async () => {
    const { root, selection } = fixture();
    selection.isCollapsed = true;
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal("document", { documentElement: {} });
    vi.stubGlobal("getComputedStyle", () => ({ getPropertyValue: () => "white" }));
    vi.spyOn(TypstDocumentContext.prototype as any, "installRescaleHandler").mockImplementation(
      () => {},
    );
    const ctx = new TypstDocumentContext({ hookedElem: root, windowElem: {}, kModule: {} } as any);
    const render = vi.fn();
    (ctx as any).r = { rescale: vi.fn(), rerender: render, postRender: vi.fn() };
    ctx.addChangement(["diff-v1", "queued"]);
    ctx.dispose();
    await frames.shift()!(0);
    expect(render).not.toHaveBeenCalled();
    expect(ctx.isRendering).toBe(false);
  });
});

describe("native visual viewport", () => {
  it("clips and offsets the visible range after pinch zoom", () => {
    expect(
      visibleVerticalBounds({ top: 40, bottom: 800 }, { offsetTop: 200, height: 300 }),
    ).toEqual({ top: 200, bottom: 500 });
  });
  it("retains desktop geometry and tolerates a transient nonintersection", () => {
    const bounds = { top: 40, bottom: 800 };
    expect(visibleVerticalBounds(bounds)).toEqual(bounds);
    expect(visibleVerticalBounds(bounds, { offsetTop: 900, height: 50 })).toEqual(bounds);
  });
});
