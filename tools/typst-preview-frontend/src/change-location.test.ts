import { describe, expect, it } from "vitest";
import { ChangeLocationQueue, parseChangeLocation } from "./change-location";

describe("last change restoration", () => {
  it("applies a resume only after rendering, once", () => {
    const queue = new ChangeLocationQueue(false);
    queue.queue([120, 0, 50], true);
    expect(queue.lastLocation).toBeUndefined();
    expect(queue.afterRender(5000, -Infinity)).toEqual({ location: [120, 0, 50], deferred: false });
    expect(queue.afterRender(6000, -Infinity)).toBeUndefined();
    expect(queue.lastLocation).toEqual([120, 0, 50]);
  });
  it("defers a resume after a recent reader gesture", () => {
    const queue = new ChangeLocationQueue(false);
    queue.queue([120, 0, 50], true);
    expect(queue.afterRender(5000, 4500)?.deferred).toBe(true);
    expect(queue.lastLocation).toEqual([120, 0, 50]);
  });
  it("does not override a theme transition but still follows subsequent live edits", () => {
    const queue = new ChangeLocationQueue(true);
    queue.queue([120, 0, 50], true);
    expect(queue.afterRender(5000, -Infinity)).toBeUndefined();
    queue.queue([121, 0, 80]);
    expect(queue.afterRender(6000, -Infinity)?.location).toEqual([121, 0, 80]);
  });
  it("coalesces hints and forgets the prior connection on reset", () => {
    const queue = new ChangeLocationQueue(false);
    queue.queue([120, 0, 50], true);
    queue.queue([121, 0, 80]);
    expect(queue.afterRender(5000, -Infinity)?.location).toEqual([121, 0, 80]);
    queue.queue([122, 0, 0]);
    queue.clear();
    expect(queue.afterRender(6000, -Infinity)).toBeUndefined();
    expect(queue.lastLocation).toBeUndefined();
  });
  it("parses only valid positive page coordinates", () => {
    expect(parseChangeLocation("120 0 50")).toEqual([120, 0, 50]);
    for (const invalid of [
      "",
      "1 2",
      "0 0 0",
      "1.2 0 0",
      "1 -1 0",
      "1 0 -1",
      "1 NaN 0",
      "1 0 Infinity",
      "1 2 3 4",
    ]) {
      expect(parseChangeLocation(invalid)).toBeUndefined();
    }
  });
});
