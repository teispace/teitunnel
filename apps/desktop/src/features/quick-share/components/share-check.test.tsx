import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Verification } from "@/lib/ipc/bindings";
import { ShareCheck } from "./share-check";

const connecting: Verification = {
  hostname: "files.teispace.com",
  status: null,
  failure: { type: "tunnelMismatch" },
  message: { key: "core.verify.tunnelMismatch", args: {} },
  protected: false,
  eventStream: false,
  links: null,
  transient: true,
};

const props = { via: "route" as const, sending: false, onCheck: async () => {}, checking: false };

describe("ShareCheck", () => {
  it("says Cloudflare is still connecting a new address instead of showing an error", () => {
    render(<ShareCheck check={connecting} settling {...props} />);
    expect(screen.getByRole("status").textContent).toMatch(/still connecting/);
  });

  it("shows the failure once it stops settling", () => {
    render(<ShareCheck check={connecting} {...props} />);
    expect(screen.queryByText(/still connecting/)).toBeNull();
  });
});
