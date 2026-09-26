import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { docDates, gitDates, screenshotsIn } from "./page-facts.ts";

describe("page facts", () => {
  it("finds each screenshot a page shows, in both color schemes, once", () => {
    const mdx = `
<Screenshot name="routes" alt="Routes" />
Some text.
<Screenshot narrow name="move-computer" alt="Settings" />
<Screenshot name="routes" alt="Again" />`;
    assert.deepEqual(screenshotsIn(mdx), [
      "https://teitunnel.teispace.com/screens/routes-light.webp",
      "https://teitunnel.teispace.com/screens/routes-dark.webp",
      "https://teitunnel.teispace.com/screens/move-computer-light.webp",
      "https://teitunnel.teispace.com/screens/move-computer-dark.webp",
    ]);
    assert.deepEqual(screenshotsIn("No pictures here."), []);
  });

  it("dates a page from its git history, first commit to last", () => {
    const dates = docDates("guides/protection.mdx");
    assert.ok(dates.published && dates.modified, "the page is in git");
    assert.ok(Date.parse(dates.published) <= Date.parse(dates.modified));
    assert.deepEqual(gitDates("content/docs/no-such-page.mdx"), {}, "no history, no dates");
  });
});
