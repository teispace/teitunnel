import { describe, expect, it } from "vitest";
import { describeAllowed, formatAllowed, parseAllowed } from "./access";

describe("who can sign in", () => {
  it("reads emails and domains however they're separated", () => {
    expect(parseAllowed(" me@xyz.com, @team.io\nyou@yx.com;corp.com ")).toEqual({
      emails: ["me@xyz.com", "you@yx.com"],
      emailDomains: ["team.io", "corp.com"],
    });
    expect(parseAllowed("  ")).toEqual({ emails: [], emailDomains: [] });
  });

  it("round-trips through the text field", () => {
    const rule = { emails: ["me@xyz.com"], emailDomains: ["team.io"] };
    expect(parseAllowed(formatAllowed(rule))).toEqual(rule);
    expect(formatAllowed(null)).toBe("");
  });

  it("describes a rule in words", () => {
    expect(describeAllowed({ emails: ["me@xyz.com"], emailDomains: [] })).toBe("me@xyz.com");
    expect(describeAllowed({ emails: ["a@x.com", "b@x.com"], emailDomains: ["team.io"] })).toBe(
      "a@x.com, b@x.com and anyone at @team.io",
    );
  });
});
