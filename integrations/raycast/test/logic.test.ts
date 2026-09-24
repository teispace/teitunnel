import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { DoctorIssue, ShareInfo } from "../src/control-client/index.ts";
import { parseOrigin, shareRow, sortIssues, sortShares } from "../src/logic.ts";

const share = (over: Partial<ShareInfo>): ShareInfo => ({
  id: "qs-1",
  kind: "quick",
  url: "https://calm-river.trycloudflare.com",
  origin: "http://localhost:3000",
  status: "live",
  startedAt: 1,
  expiresAt: null,
  requests: 4,
  accountId: null,
  ...over,
});

describe("Raycast commands", () => {
  it("accept ports as typed", () => {
    assert.equal(parseOrigin("3000"), "3000");
    assert.equal(parseOrigin(":5173"), "5173");
    assert.equal(parseOrigin("localhost:8080"), "localhost:8080");
    assert.equal(parseOrigin("http://127.0.0.1:3000/"), "http://127.0.0.1:3000/");
    assert.equal(parseOrigin("0"), undefined);
    assert.equal(parseOrigin("my site"), undefined);
  });

  it("describe and order shares", () => {
    const row = shareRow(share({}));
    assert.deepEqual([row.title, row.subtitle, row.status], [
      "calm-river.trycloudflare.com",
      "localhost:3000 · Quick Share",
      "4 requests",
    ]);
    assert.equal(shareRow(share({ url: null, status: "starting" })).title, "Waiting for an address…");
    const ordered = sortShares([
      share({ id: "old", startedAt: 1 }),
      share({ id: "starting", status: "starting", startedAt: 9 }),
      share({ id: "new", startedAt: 5 }),
    ]).map((s) => s.id);
    assert.deepEqual(ordered, ["new", "old", "starting"]);
  });

  it("put errors first", () => {
    const issue = (id: string, severity: string): DoctorIssue => ({
      id,
      check: "c",
      severity,
      accountId: null,
      subject: "s",
      title: id,
      detail: "d",
    });
    assert.deepEqual(
      sortIssues([issue("i", "info"), issue("e", "error"), issue("w", "warning")]).map((i) => i.id),
      ["e", "w", "i"],
    );
  });
});
