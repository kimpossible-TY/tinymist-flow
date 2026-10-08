import { describe, expect, it, vi } from "vitest";
import { ReviewMarks } from "./review-marks";
import { pageFragment, type SelectedText } from "./text-selection";

const selection: SelectedText = {
  text: "선택한 text",
  start: { page_no: 1, x: 10, y: 20 },
  end: { page_no: 2, x: 20, y: 30 },
  fragments: [
    { page: 1, x: 10, y: 20, width: 60, height: 12 },
    { page: 2, x: 20, y: 30, width: 40, height: 12 },
  ],
  fingerprints: { 1: "first", 2: "second" },
};

function storage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
}

describe("temporary review marks", () => {
  it("persists by document, restores across themes, and supports undo/clear", () => {
    const saved = storage();
    const marks = new ReviewMarks(saved);
    marks.setDocument("project-a");
    marks.add(selection);
    const refreshed = new ReviewMarks(saved);
    refreshed.setDocument("project-a");
    expect(refreshed.marks).toHaveLength(1);
    refreshed.setDocument("project-a");
    expect(refreshed.marks).toHaveLength(1);
    refreshed.setDocument("project-b");
    expect(refreshed.marks).toHaveLength(0);
    refreshed.setDocument("project-a");
    refreshed.undo();
    expect(refreshed.marks).toHaveLength(0);
    refreshed.add(selection);
    refreshed.clear();
    expect(refreshed.marks).toHaveLength(0);
    const again = new ReviewMarks(saved);
    again.setDocument("project-a");
    expect(again.marks).toHaveLength(0);
  });

  it("retains virtual pages and discards changed layout across multi-page ranges", () => {
    const marks = new ReviewMarks();
    marks.add(selection);
    marks.reconcile(
      new Map([
        [1, { fingerprint: "first" }],
        [2, { fingerprint: "" }],
      ]),
    );
    expect(marks.marks).toHaveLength(1);
    marks.reconcile(new Map());
    expect(marks.marks).toHaveLength(1);
    marks.reconcile(new Map([[2, { fingerprint: "moved" }]]));
    expect(marks.marks).toHaveLength(0);
  });

  it("bounds records and tolerates corrupt/unavailable browser storage", () => {
    const saved = storage();
    const marks = new ReviewMarks(saved);
    marks.setDocument("project");
    for (let i = 0; i < 105; i++) marks.add(selection);
    expect(marks.marks).toHaveLength(100);
    saved.setItem("tinymist-review-v1:project", '[{"fragments":[{"page":1,"x":null}]}]');
    const bad = new ReviewMarks(saved);
    bad.setDocument("project");
    expect(bad.marks).toHaveLength(0);
    const unavailable = new ReviewMarks({
      getItem: vi.fn(() => {
        throw new Error("blocked");
      }),
      setItem: vi.fn(() => {
        throw new Error("quota");
      }),
    });
    unavailable.setDocument("project");
    unavailable.add(selection);
    expect(unavailable.marks).toHaveLength(1);
  });

  it("uses page coordinates that remain the same after resize and scroll", () => {
    const geometry = {
      width: 600,
      height: 800,
      rect: { left: 10, top: -300, width: 300, height: 400 } as DOMRect,
    };
    const expected = { page: 1, x: 20, y: 40, width: 60, height: 12 };
    expect(pageFragment(1, { left: 20, top: -280, width: 30, height: 6 }, geometry)).toEqual(
      expected,
    );
    geometry.rect = { left: 40, top: -200, width: 900, height: 1200 } as DOMRect;
    expect(pageFragment(1, { left: 70, top: -140, width: 90, height: 18 }, geometry)).toEqual(
      expected,
    );
  });
});
