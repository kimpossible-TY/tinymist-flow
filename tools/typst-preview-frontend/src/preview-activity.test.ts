import { describe, expect, it } from "vitest";
import { activityPlacement, PreviewActivityState } from "./preview-activity";

describe("preview activity lifetime", () => {
  it("waits for an actual document render, not socket or compiler success", () => {
    const state = new PreviewActivityState();
    expect(state.phase).toBe("preparing");
    state.connecting();
    expect(state.phase).toBe("connecting");
    state.opened();
    state.didRender(); // Initial viewport work has no document.
    expect(state.phase).toBe("waiting");
    state.compile("compiling");
    expect(state.phase).toBe("compiling");
    state.compile("success");
    expect(state.phase).toBe("waiting");
    state.frame();
    expect(state.phase).toBe("rendering");
    state.didRender();
    expect(state.phase).toBe("ready");
  });

  it("does not clear live compilation or its failure on viewport work", () => {
    const state = new PreviewActivityState();
    state.opened();
    state.frame();
    state.didRender();
    state.compile("compiling");
    state.didRender();
    expect(state.phase).toBe("compiling");
    state.compile("error");
    state.didRender();
    expect(state.phase).toBe("compile-error");
    state.compile("unknown");
    expect(state.phase).toBe("compile-error");
    state.frame(); // A new viewer can still receive the last good document.
    state.didRender();
    expect(state.phase).toBe("compile-error");
    state.compile("compiling");
    state.compile("success");
    state.frame();
    state.didRender();
    expect(state.phase).toBe("ready");
  });

  it("accepts compiler success delivered after the new document renders", () => {
    const state = new PreviewActivityState();
    state.opened();
    state.compile("compiling");
    state.frame();
    state.didRender();
    expect(state.phase).toBe("compiling");
    state.compile("success");
    expect(state.phase).toBe("ready");
  });

  it("reports selection-held updates until release and completed rendering", () => {
    const state = new PreviewActivityState();
    state.opened();
    state.frame();
    state.renderActivity("selection-held");
    expect(state.phase).toBe("selection-held");
    state.renderActivity("rendering");
    expect(state.phase).toBe("rendering");
    state.didRender();
    expect(state.phase).toBe("ready");
    state.renderActivity("selection-held");
    expect(state.phase).toBe("ready");
  });

  it("keeps reconnecting despite a stale render completion", () => {
    const state = new PreviewActivityState();
    state.opened();
    state.frame();
    state.disconnected();
    state.didRender();
    state.renderError();
    expect(state.phase).toBe("reconnecting");
    state.opened();
    expect(state.phase).toBe("waiting");
    state.frame();
    state.didRender();
    expect(state.phase).toBe("ready");
  });

  it("reports initialization and rendering failures and recovers on a fresh frame", () => {
    const state = new PreviewActivityState();
    state.renderError();
    expect(state.phase).toBe("render-error");
    state.opened();
    state.frame();
    state.renderError();
    expect(state.phase).toBe("render-error");
    state.didRender();
    expect(state.phase).toBe("render-error");
    state.frame();
    state.didRender();
    expect(state.phase).toBe("ready");
  });

  it("keeps status inside a zoomed safe viewport without scaling the document", () => {
    expect(
      activityPlacement(
        { offsetLeft: 100, offsetTop: 200, width: 196.5, scale: 2 },
        { left: 0, right: 0, top: 44 },
      ),
    ).toEqual({
      left: "198.25px",
      top: "242px",
      maxWidth: "369px",
      transform: "translateX(-50%) scale(0.5)",
      transformOrigin: "top center",
    });
  });
});
