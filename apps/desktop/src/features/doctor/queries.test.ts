import { describe, expect, it } from "vitest";
import { rawText } from "@/lib/i18n";
import type { Issue, Severity } from "@/lib/ipc/bindings";
import { bySeverity } from "./queries";

const issue = (id: string, severity: Severity): Issue => ({
  id,
  check: id,
  severity,
  accountId: null,
  tunnelId: null,
  subject: id,
  label: rawText(id),
  title: rawText(id),
  detail: rawText(id),
  evidence: [],
  fixes: [],
});

describe("bySeverity", () => {
  it("keeps each severity together when a later source adds more", () => {
    // Cloudflare findings (sorted), then local-domain findings appended after them.
    const list = [
      issue("cf-error", "error"),
      issue("cf-warning", "warning"),
      issue("local-warning", "warning"),
      issue("local-error", "error"),
      issue("cf-info", "info"),
    ];
    expect(bySeverity(list).map((i) => i.id)).toEqual([
      "cf-error",
      "local-error",
      "cf-warning",
      "local-warning",
      "cf-info",
    ]);
  });
});
