import assert from "node:assert/strict";
import { test } from "node:test";
import { HostError, type Share } from "../src/host.ts";
import { explain, shareFor, shareLabel, shareTone } from "../src/view.ts";

const share = (extra: Partial<Share>): Share => ({
  id: "qs-1",
  kind: "quick",
  url: "https://quiet-river.trycloudflare.com",
  origin: "http://localhost:5173",
  status: "live",
  ...extra,
});

test("finds the share already serving the page", () => {
  const shares = [
    share({}),
    share({ id: "qs-2", origin: "http://localhost:3000", status: "failed" }),
  ];
  assert.equal(shareFor("http://127.0.0.1:5173", shares)?.id, "qs-1");
  assert.equal(shareFor("http://localhost:3000", shares), null, "a failed one doesn't count");
  assert.equal(shareFor(null, shares), null);
});

test("labels shares and explains errors", () => {
  assert.equal(shareLabel(share({})), "quiet-river.trycloudflare.com");
  assert.equal(shareLabel(share({ url: null, status: "starting" })), "Getting a URL…");
  assert.equal(shareTone(share({ paused: true })), "paused");
  assert.equal(explain(new HostError("appNotRunning", "x")).action, "openApp");
  assert.equal(explain(new HostError("hostMissing", "Set it up")).action, "setUp");
  assert.equal(explain(new HostError("declined", "x")).text, "Not allowed in Teitunnel.");
});
