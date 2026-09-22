import { describe, expect, it } from "vitest";
import { cn } from "./cn";

describe("cn", () => {
  it("keeps token utilities from different groups", () => {
    expect(cn("text-body text-primary")).toBe("text-body text-primary");
    expect(cn("border-hairline border-control")).toBe("border-hairline border-control");
    expect(cn("rounded-card shadow-raised")).toBe("rounded-card shadow-raised");
  });

  it("still resolves real conflicts", () => {
    expect(cn("text-body", "text-callout")).toBe("text-callout");
    expect(cn("text-primary", "text-secondary")).toBe("text-secondary");
    expect(cn("h-6", "h-7")).toBe("h-7");
  });
});
