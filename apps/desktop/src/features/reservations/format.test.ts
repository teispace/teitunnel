import { describe, expect, it } from "vitest";
import { availabilityStatus, checkable, holdText, ownerName } from "./format";

describe("reservation text", () => {
  it("says who holds a name, and until when", () => {
    expect(
      holdText({ hostname: "a.xyz.com", owner: "bob@pc", until: null, kind: "reservation" }),
    ).toBe("Reserved by bob@pc");
    expect(holdText({ hostname: "a.xyz.com", owner: null, until: null, kind: "route" })).toBe(
      "Routed by another Teitunnel on another machine",
    );
    const until = holdText({
      hostname: "a.xyz.com",
      owner: "bob@pc",
      until: Date.parse("2026-12-31T12:00:00Z"),
      kind: "reservation",
    });
    expect(until).toMatch(/^Reserved by bob@pc until .*2026/);
    expect(ownerName(null)).toBe("another Teitunnel");
  });

  it("maps availability to a dot and a line", () => {
    expect(availabilityStatus({ state: "free" })).toEqual({ dot: "healthy", label: "Available" });
    expect(availabilityStatus({ state: "yours" })?.label).toBe("Reserved by you");
    expect(availabilityStatus({ state: "foreign" })?.dot).toBe("warning");
    expect(availabilityStatus({ state: "noZone" })).toBeNull();
    expect(
      availabilityStatus({
        state: "held",
        hold: { hostname: "a.xyz.com", owner: "bob@pc", until: null, kind: "route" },
      }),
    ).toEqual({ dot: "warning", label: "Routed by bob@pc on another machine" });
  });

  it("only checks whole hostnames", () => {
    expect(checkable("app.xyz.com")).toBe(true);
    expect(checkable(" App.XYZ.com ")).toBe(true);
    expect(checkable("*.xyz.com")).toBe(true);
    for (const partial of ["", "app", "app.", ".xyz.com", "app..xyz.com", "a b.xyz.com"]) {
      expect(checkable(partial), partial).toBe(false);
    }
  });
});
