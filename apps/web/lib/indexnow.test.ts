import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { changedSince, submission } from "./indexnow.ts";

const sitemap = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
<url><loc>https://teitunnel.teispace.com/</loc><lastmod>2026-09-26T10:00:00Z</lastmod></url>
<url><loc>https://teitunnel.teispace.com/docs/old/</loc><lastmod>2026-09-01T10:00:00Z</lastmod></url>
<url><loc>https://teitunnel.teispace.com/docs/undated/</loc></url>
</urlset>`;

describe("IndexNow", () => {
  it("submits only the pages that changed", () => {
    assert.deepEqual(changedSince(sitemap, new Date("2026-09-25T10:00:00Z")), [
      "https://teitunnel.teispace.com/",
    ]);
    assert.deepEqual(changedSince(sitemap, new Date("2026-09-30T00:00:00Z")), []);
  });

  it("points at the key file on the same host", () => {
    const body = submission("teitunnel.teispace.com", "abc", ["https://teitunnel.teispace.com/"]);
    assert.equal(body.keyLocation, "https://teitunnel.teispace.com/abc.txt");
    assert.deepEqual(body.urlList, ["https://teitunnel.teispace.com/"]);
  });
});
