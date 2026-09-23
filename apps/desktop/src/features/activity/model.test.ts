import { describe, expect, it } from "vitest";
import type { ActivityEntry, ActivityRecord } from "@/lib/ipc/bindings";
import { commandScript, inZone, matches } from "./model";

function entry(
  overrides: Partial<Omit<ActivityEntry, "record">> & { record?: Partial<ActivityRecord> | null },
): ActivityEntry {
  const { record, ...rest } = overrides;
  return {
    id: 1,
    at: 0,
    summary: "Add app.xyz.com → http://localhost:3000",
    outcome: "applied",
    detail: ["Add DNS record app.xyz.com → tunnel “Mac”"],
    record:
      record === null
        ? null
        : {
            kind: "addRoute",
            hostnames: ["app.xyz.com"],
            tunnel: "Mac",
            steps: [],
            changes: [],
            ...record,
          },
    ...rest,
  };
}

const all = { show: "all", zone: null, query: "" } as const;

describe("matches", () => {
  it("filters by outcome and kind", () => {
    expect(matches(entry({}), all)).toBe(true);
    expect(matches(entry({}), { ...all, show: "problems" })).toBe(false);
    expect(matches(entry({ outcome: "rolledBack" }), { ...all, show: "problems" })).toBe(true);
    expect(matches(entry({}), { ...all, show: "addRoute" })).toBe(true);
    expect(matches(entry({}), { ...all, show: "removeRoute" })).toBe(false);
    // Older entries have no kind, so they only show under All and Problems.
    expect(matches(entry({ record: null }), { ...all, show: "addRoute" })).toBe(false);
  });

  it("filters by domain, including subdomains but not look-alikes", () => {
    expect(matches(entry({}), { ...all, zone: "xyz.com" })).toBe(true);
    expect(matches(entry({}), { ...all, zone: "yz.com" })).toBe(false);
    expect(matches(entry({ record: null }), { ...all, zone: "xyz.com" })).toBe(true);
  });

  it("searches the summary, steps and hostnames", () => {
    expect(matches(entry({}), { ...all, query: "localhost:3000" })).toBe(true);
    expect(matches(entry({}), { ...all, query: "tunnel “mac”" })).toBe(true);
    expect(
      matches(entry({ summary: "Import 2 routes", record: { hostnames: ["api.yx.dev"] } }), {
        ...all,
        query: "api.yx",
      }),
    ).toBe(true);
    expect(matches(entry({}), { ...all, query: "nothing" })).toBe(false);
  });
});

describe("inZone", () => {
  it("matches the apex and subdomains only", () => {
    expect(inZone("xyz.com", "xyz.com")).toBe(true);
    expect(inZone("a.b.xyz.com", "xyz.com")).toBe(true);
    expect(inZone("axyz.com", "xyz.com")).toBe(false);
  });
});

describe("commandScript", () => {
  it("joins commands under their descriptions", () => {
    expect(
      commandScript([
        {
          kind: "createTunnel",
          description: "Create tunnel “Mac”",
          command: "cloudflared tunnel create 'Mac'",
        },
        { kind: "stopConnector", description: "Stop this Mac's connector", command: null },
        {
          kind: "verify",
          description: "Check https://a.xyz.com works",
          command: "curl -I https://a.xyz.com",
        },
      ]),
    ).toBe(
      "# Create tunnel “Mac”\ncloudflared tunnel create 'Mac'\n\n# Check https://a.xyz.com works\ncurl -I https://a.xyz.com\n",
    );
    expect(commandScript([])).toBeNull();
  });
});
