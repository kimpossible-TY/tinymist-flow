import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { focusStatusPlacement } from "./mobile-viewport";

describe("mobile viewport", () => {
  const viewport = { offsetLeft: 0, offsetTop: 0, width: 852, height: 393, scale: 1 };
  it("keeps landscape feedback clear of the notch and home indicator", () => {
    const style = focusStatusPlacement(viewport, { left: 59, right: 59, bottom: 21 });
    expect(style.left).toBe("426px");
    expect(style.top).toBe("372px");
    expect(style.maxWidth).toBe("734px");
  });
  it("centers feedback inside an asymmetric safe area", () => {
    const style = focusStatusPlacement(viewport, { left: 59, right: 0, bottom: 0 });
    expect(style.left).toBe("449.5px");
    expect(style.maxWidth).toBe("781px");
    expect(style.top).toBe("381px");
  });
  it("keeps feedback readable during native zoom and pan", () => {
    const style = focusStatusPlacement(
      { offsetLeft: 100, offsetTop: 50, width: 426, height: 196.5, scale: 2 },
      { left: 59, right: 59, bottom: 21 },
    );
    expect(style.left).toBe("313px");
    expect(style.top).toBe("236px");
    expect(style.maxWidth).toBe("734px");
    expect(style.transform).toBe("translate(-50%, -100%) scale(0.5)");
  });
  it("retains the desktop feedback gutter", () => {
    const style = focusStatusPlacement(viewport, { left: 0, right: 0, bottom: 0 });
    expect(style.maxWidth).toBe("828px");
    expect(style.top).toBe("381px");
  });
  it("opts into edge-to-edge layout without disabling native zoom", () => {
    const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
    const meta = html.match(/<meta name="viewport"[^>]+>/)?.[0];
    expect(meta).toContain("viewport-fit=cover");
    expect(meta).not.toMatch(/user-scalable|maximum-scale/);
  });
  it("uses dynamic height with a legacy fallback in document and slide mode", () => {
    const css = readFileSync(new URL("./typst.css", import.meta.url), "utf8");
    const layout = readFileSync(new URL("./styles/layout.css", import.meta.url), "utf8");
    expect(css).toMatch(/height: 100vh;\s*height: 100dvh;/);
    expect(css).toContain("min-height: 0;");
    expect(layout).toMatch(/height: 100vh;\s*height: 100dvh;/);
    expect(layout).toMatch(/max-height: 100vh;\s*max-height: 100dvh;/);
  });
});
