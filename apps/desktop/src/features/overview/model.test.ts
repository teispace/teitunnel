import { describe, expect, it } from "vitest";
import type { UptimeSummary } from "@/lib/ipc/bindings";
import { emptySeries } from "@/lib/traffic";
import { errorTone, recentErrors, uptimeGlance } from "./model";

const NOW = 1_000_000_000;

function series(
  samples: { ageS: number; requests: number; serverErrors?: number; errors?: number }[],
) {
  const s = emptySeries();
  for (const sample of samples) {
    s.at.push(NOW - sample.ageS * 1000);
    s.span.push(1);
    s.requests.push(sample.requests);
    s.serverErrors.push(sample.serverErrors ?? 0);
    s.errors.push(sample.errors ?? 0);
  }
  return s;
}

function uptime(hostname: string, patch: Partial<UptimeSummary>): UptimeSummary {
  return {
    accountId: "acc",
    route: { hostname, path: null },
    up: true,
    lastChecked: NOW,
    lastLatencyMs: 40,
    lastCause: null,
    uptimeDay: 1,
    uptimeWeek: 1,
    uptimeMonth: 1,
    p95Ms: 80,
    openIncident: null,
    ...patch,
  };
}

describe("the Overview at a glance", () => {
  it("counts 5xx and unreachable requests of the last five minutes only", () => {
    const s = series([
      { ageS: 600, requests: 50, serverErrors: 50 },
      { ageS: 200, requests: 10, serverErrors: 1 },
      { ageS: 10, requests: 10, errors: 2 },
    ]);
    expect(recentErrors(s, 300, NOW)).toEqual({ requests: 20, failed: 3 });
    expect(recentErrors(series([{ ageS: 600, requests: 5 }]), 300, NOW)).toBeNull();
    // An unreachable request the connector also counted as a 5xx isn't counted twice over.
    expect(
      recentErrors(series([{ ageS: 1, requests: 1, serverErrors: 1, errors: 1 }]), 300, NOW),
    ).toEqual({ requests: 1, failed: 1 });
  });

  it("warns on any failure and alarms over 5%", () => {
    expect(errorTone(null)).toBe("neutral");
    expect(errorTone({ requests: 0, failed: 0 })).toBe("neutral");
    expect(errorTone({ requests: 100, failed: 0 })).toBe("healthy");
    expect(errorTone({ requests: 100, failed: 5 })).toBe("warning");
    expect(errorTone({ requests: 100, failed: 6 })).toBe("error");
  });

  it("sums up the account's checked routes", () => {
    const list = [
      uptime("a.xyz.com", { uptimeDay: 0.999 }),
      uptime("b.xyz.com", { up: false, uptimeDay: 0.9 }),
      uptime("new.xyz.com", { up: null, uptimeDay: null }),
      uptime("other.com", { accountId: "other", up: false, uptimeDay: 0.1 }),
    ];
    const glance = uptimeGlance(list, "acc");
    expect(glance?.checked).toBe(2);
    expect(glance?.down.map((u) => u.route.hostname)).toEqual(["b.xyz.com"]);
    expect(glance?.lowestDay).toBe(0.9);
    expect(uptimeGlance([uptime("n.xyz.com", { up: null })], "acc")).toBeNull();
    expect(uptimeGlance(list, null)).toBeNull();
  });
});
