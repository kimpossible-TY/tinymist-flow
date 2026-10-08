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
  it("does not jump back to a saved page 2 after reconnecting a rendered viewer", () => {
    const queue = new ChangeLocationQueue(false);
    queue.queue([2, 0, 50], true);
    expect(queue.afterRender(5000, -Infinity)?.location).toEqual([2, 0, 50]);

    for (let retry = 0; retry < 2; retry++) {
      queue.clear();
      queue.queue([2, 0, 50], true);
      expect(queue.afterRender(6000 + retry * 1000, 5500 + retry * 1000)).toBeUndefined();
      expect(queue.lastLocation).toBeUndefined();
    }

    queue.queue([121, 0, 80]);
    expect(queue.afterRender(9000, -Infinity)).toEqual({
      location: [121, 0, 80],
      deferred: false,
    });
    queue.queue([122, 0, 40]);
    expect(queue.afterRender(10000, 9500)?.deferred).toBe(true);
  });
  it("preserves a rendered viewer even if its initial document had no saved edit", () => {
    const queue = new ChangeLocationQueue(false);
    expect(queue.afterRender(5000, -Infinity)).toBeUndefined();
    queue.clear();
    queue.queue([2, 0, 0], true);
    expect(queue.afterRender(6000, -Infinity)).toBeUndefined();
  });
  it("still restores an initial saved edit after a connection fails before rendering", () => {
    const queue = new ChangeLocationQueue(false);
    queue.queue([2, 0, 50], true);
    queue.clear();
    queue.queue([2, 0, 50], true);
    expect(queue.afterRender(5000, -Infinity)?.location).toEqual([2, 0, 50]);
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
