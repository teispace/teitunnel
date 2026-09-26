import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { faqFrom, plainText } from "./faq.ts";

describe("FAQ structured data", () => {
  it("reads questions and answers from accordions, as plain text", () => {
    const mdx = `
<Accordions>
  <Accordion title="Is Teitunnel made by Cloudflare?">
    No. It's an **independent** project that runs \`cloudflared\`, see
    [Install](/docs/getting-started/install).
  </Accordion>
  <Accordion title="Does it cost anything?">
    No.
  </Accordion>
</Accordions>`;
    assert.deepEqual(faqFrom(mdx), [
      {
        q: "Is Teitunnel made by Cloudflare?",
        a: "No. It's an independent project that runs cloudflared, see Install.",
      },
      { q: "Does it cost anything?", a: "No." },
    ]);
    assert.equal(plainText("```sh\nsecret\n```\nDone."), "Done.");
  });
});
