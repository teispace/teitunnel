import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { revealChange } from "./reveal.ts";

const base = { shown: false, visible: 0, height: 400, top: 900, viewport: 800 };

describe("scroll reveal", () => {
  it("shows once enough of it is in view", () => {
    assert.equal(revealChange({ ...base, visible: 20, top: 780 }), null);
    assert.equal(revealChange({ ...base, visible: 60, top: 740 }), "show");
  });

  it("measures tall elements against the window", () => {
    // 100 px of a 3000 px element is 3% of it, but 12.5% of the window.
    assert.equal(revealChange({ ...base, height: 3000, visible: 100, top: 700 }), "show");
  });

  it("hides again as it leaves through the bottom", () => {
    assert.equal(revealChange({ ...base, shown: true, visible: 10, top: 790 }), "hide");
  });

  it("stays shown as it leaves through the top", () => {
    assert.equal(revealChange({ ...base, shown: true, visible: 0, top: -400 }), null);
  });

  it("doesn't flicker between the thresholds", () => {
    assert.equal(revealChange({ ...base, shown: true, visible: 30, top: 770 }), null);
    assert.equal(revealChange({ ...base, shown: false, visible: 30, top: 770 }), null);
  });
});
