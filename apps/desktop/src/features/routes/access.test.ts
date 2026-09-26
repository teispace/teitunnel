import { describe, expect, it } from "vitest";
import { describeAllowed, formatAllowed, formatPaths, parseAllowed, parsePaths } from "./access";

describe("who can sign in", () => {
  it("reads emails and domains however they're separated", () => {
    expect(parseAllowed(" me@xyz.com, @team.io\nyou@yx.com;corp.com other@yx.com ")).toEqual({
      emails: ["me@xyz.com", "you@yx.com", "other@yx.com"],
      emailDomains: ["team.io", "corp.com"],
      github: [],
    });
    expect(parseAllowed("  ")).toEqual({ emails: [], emailDomains: [], github: [] });
  });

  it("keeps a GitHub team's name whole, spaces included", () => {
    expect(parseAllowed("GitHub:teispace, github: softup-llc/Softup Dev\nme@xyz.com")).toEqual({
      emails: ["me@xyz.com"],
      emailDomains: [],
      github: ["teispace", "softup-llc/Softup Dev"],
    });
  });

  it("round-trips through the text field", () => {
    const rule = {
      emails: ["me@xyz.com"],
      emailDomains: ["team.io"],
      github: ["teispace/Softup Dev"],
    };
    expect(parseAllowed(formatAllowed(rule))).toEqual(rule);
    expect(formatAllowed(null)).toBe("");
  });

  it("describes a rule in words", () => {
    expect(describeAllowed({ emails: ["me@xyz.com"], emailDomains: [] })).toBe("me@xyz.com");
    expect(describeAllowed({ emails: ["a@x.com", "b@x.com"], emailDomains: ["team.io"] })).toBe(
      "a@x.com, b@x.com, and anyone at @team.io",
    );
    expect(describeAllowed({ emails: [], emailDomains: [], github: ["teispace/devs"] })).toBe(
      "members of teispace/devs on GitHub",
    );
  });
});

describe("paths that skip a login", () => {
  it("reads and writes them as typed", () => {
    expect(parsePaths(" /webhooks, /api/hooks\n")).toEqual(["/webhooks", "/api/hooks"]);
    expect(parsePaths("")).toEqual([]);
    expect(formatPaths(["/webhooks", "/api/hooks"])).toBe("/webhooks, /api/hooks");
    expect(formatPaths(undefined)).toBe("");
  });
});
