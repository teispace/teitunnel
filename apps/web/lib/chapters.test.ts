import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { tourPosition } from "./chapters.ts";

describe("tour position", () => {
  it("starts at the first card before any has reached the line", () => {
    assert.deepEqual(tourPosition([700, 1600, 2500], 190, 900), { active: 0, progress: 0 });
  });

  it("marks the last card that has reached the line", () => {
    // Stuck cards share a top; the third is still below the line.
    assert.equal(tourPosition([124, 124, 400], 190, 900).active, 1);
  });

  it("counts how far the next card has come up", () => {
    // The first card is on top and the second is halfway from the bottom to the line.
    const { active, progress } = tourPosition([124, 545, 1500], 190, 900);
    assert.equal(active, 0);
    assert.equal(progress, 0.25);
  });

  it("ends at 1 on the last card", () => {
    assert.deepEqual(tourPosition([124, 124, 124], 190, 900), { active: 2, progress: 1 });
  });

  it("stays at 0 with one card", () => {
    assert.deepEqual(tourPosition([124], 190, 900), { active: 0, progress: 0 });
  });
});
