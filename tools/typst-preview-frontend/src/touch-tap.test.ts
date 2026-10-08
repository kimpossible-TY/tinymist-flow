import { describe, expect, it } from "vitest";
import { TouchTap } from "./touch-tap";

describe("source tap classification", () => {
  it("accepts a short stationary tap once", () => {
    const tap = new TouchTap();
    tap.begin(10, 20, 0, 1, false);
    expect(tap.consume(100, false)).toBe(true);
    expect(tap.consume(110, false)).toBe(false);
  });

  it.each(["long", "drag", "cancel", "multi", "existing", "new range"])(
    "rejects %s gestures",
    (kind) => {
      const tap = new TouchTap();
      tap.begin(10, 20, 0, kind === "multi" ? 2 : 1, kind === "existing");
      if (kind === "drag") {
        tap.move(10, 40, 1);
        tap.move(10, 20, 1); // Returning to the start does not undo the drag.
      }
      if (kind === "cancel") tap.cancel();
      expect(tap.consume(kind === "long" ? 600 : 100, kind === "new range")).toBe(false);
      tap.begin(10, 20, 700, 1, false);
      expect(tap.consume(800, false)).toBe(true);
    },
  );

  it("rejects clicks without a touch and a second finger added midgesture", () => {
    const tap = new TouchTap();
    expect(tap.consume(100, false)).toBe(false);
    tap.begin(0, 0, 200, 1, false);
    tap.move(0, 0, 2);
    expect(tap.consume(250, false)).toBe(false);
  });
});
