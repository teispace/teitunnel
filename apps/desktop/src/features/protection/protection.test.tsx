import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { PlanView, ProtectionView, ServiceTokenView } from "@/lib/ipc/bindings";
import { ProtectionSection } from "./components/protection-section";
import { ServiceTokens } from "./components/service-tokens";

const core = (key: string, args: Record<string, string | number> = {}) => ({
  key: `core.${key}`,
  args,
});

let view: ProtectionView;
let tokens: ServiceTokenView[];
let edgeRefused: boolean;
let calls: { cmd: string; args: Record<string, unknown> }[];

const plan: PlanView = {
  steps: [
    {
      kind: "edgeRule",
      description: core("protection.step.add.block", { hostname: "app.xyz.com" }),
      command: null,
    },
  ],
  warnings: [{ type: "edgeQuota", quota: "custom", zone: "xyz.com", used: 2, limit: 5 }],
  requiresConfirmation: false,
  fingerprint: "fp-1",
};

const applied = { type: "applied", tunnelId: null, verify: [], connectorError: null };

beforeEach(() => {
  view = {
    hostname: "app.xyz.com",
    zone: "xyz.com",
    plan: "free",
    protection: {
      bots: "off",
      aiCrawlers: true,
      rateLimit: null,
      requestHeaders: [],
      responseHeaders: [{ name: "X-Robots-Tag", op: "set", value: "noindex" }],
    },
    quotas: [
      { quota: "custom", used: 1, limit: 5 },
      { quota: "rateLimit", used: 0, limit: 1 },
      { quota: "transform", used: 1, limit: 10 },
    ],
    rateLimitAvailable: false,
    longestPeriod: 10,
    sharesRateLimitWith: [],
  };
  tokens = [];
  edgeRefused = false;
  calls = [];
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "accounts_list":
        return [{ id: "a1", name: "Personal", credential: "apiToken", limitedZone: null }];
      case "accounts_capabilities":
        return {
          zonesRead: "yes",
          tunnelsRead: "yes",
          tunnelsEdit: "yes",
          accessEdit: "yes",
          analytics: "yes",
          workersEdit: "yes",
          edgeRules: edgeRefused ? "no" : "yes",
          serviceTokens: "yes",
          d1: "yes",
          zones: [],
        };
      case "protection_get":
        if (edgeRefused) {
          throw {
            code: "permissionDenied",
            message: core("error.observe.edgePermission"),
            hint: null,
            field: null,
          };
        }
        return view;
      case "protection_tokens":
        return tokens;
      case "protection_preview":
        return plan;
      case "protection_apply": {
        const change = payload["change"] as { type: string };
        if (change.type === "createToken") {
          tokens = [
            {
              id: "tok1",
              label: "CI",
              clientId: "tok1.access",
              expiresAt: "2027-09-24T00:00:00Z",
              gone: false,
            },
          ];
          return {
            outcome: applied,
            issued: [
              {
                tokenId: "tok1",
                name: "Teitunnel · app.xyz.com · CI",
                clientId: "tok1.access",
                expiresAt: "2027-09-24T00:00:00Z",
              },
            ],
          };
        }
        return { outcome: applied, issued: [] };
      }
      default:
        return null;
    }
  });
});

function renderSection(node: React.ReactNode) {
  render(<QueryClientProvider client={createQueryClient()}>{node}</QueryClientProvider>);
}

describe("ProtectionSection", () => {
  it("shows what the hostname has, then reviews and applies a change", async () => {
    renderSection(<ProtectionSection accountId="a1" hostname="app.xyz.com" />);
    expect(await screen.findByText("Blocked")).toBeTruthy();
    expect(screen.getByText("Request: 0 · Response: 1")).toBeTruthy();
    expect(screen.getByText("Rules on xyz.com")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Edit Protection…" }));
    const sheet = await screen.findByRole("dialog");
    // Free zones can't limit one hostname: the option explains instead.
    expect(within(sheet).getByText(/Not on the Free plan/)).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("radio", { name: "Block" }));
    fireEvent.click(within(sheet).getByRole("button", { name: "Review" }));

    expect(await within(sheet).findByText("Block automated clients on app.xyz.com")).toBeTruthy();
    expect(
      within(sheet).getByText("xyz.com will use 2 of the 5 custom rules its plan allows."),
    ).toBeTruthy();
    expect(calls.find((c) => c.cmd === "protection_preview")?.args["change"]).toEqual({
      type: "protect",
      hostname: "app.xyz.com",
      protection: {
        bots: "block",
        aiCrawlers: true,
        rateLimit: null,
        requestHeaders: [],
        responseHeaders: [{ name: "X-Robots-Tag", op: "set", value: "noindex" }],
      },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Apply Rules" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "protection_apply")?.args["fingerprint"]).toBe("fp-1"),
    );
  });

  it("offers the permission fix when the token can't manage edge rules", async () => {
    edgeRefused = true;
    renderSection(<ProtectionSection accountId="a1" hostname="app.xyz.com" />);
    expect(await screen.findByText("Zone · Zone WAF · Edit")).toBeTruthy();
    expect(screen.getByText("Zone · Transform Rules · Edit")).toBeTruthy();
  });
});

describe("ServiceTokens", () => {
  it("creates a token and lets the secret be copied without showing it", async () => {
    renderSection(<ServiceTokens accountId="a1" hostname="app.xyz.com" />);
    expect(await screen.findByText("No service tokens.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "New Service Token…" }));
    const sheet = await screen.findByRole("dialog");
    fireEvent.click(within(sheet).getByRole("button", { name: "Review" }));
    fireEvent.click(await within(sheet).findByRole("button", { name: "Create Token" }));

    expect(await screen.findByText("Save the Secret Now")).toBeTruthy();
    const issued = screen.getByRole("dialog");
    expect(within(issued).getByText("tok1.access")).toBeTruthy();
    fireEvent.click(within(issued).getByRole("button", { name: "Copy CF-Access-Client-Secret" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "protection_copy_secret")?.args).toEqual({
        tokenId: "tok1",
        what: "secret",
      }),
    );
    fireEvent.click(within(issued).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "protection_forget_secret")).toBe(true));
    expect(await screen.findByText("CI")).toBeTruthy();
  });

  it("revokes a token after confirming", async () => {
    tokens = [{ id: "tok1", label: "CI", clientId: "tok1.access", expiresAt: null, gone: false }];
    renderSection(<ServiceTokens accountId="a1" hostname="app.xyz.com" />);
    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Revoke “CI”?")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "protection_apply")?.args["change"]).toEqual({
        type: "revokeToken",
        hostname: "app.xyz.com",
        tokenId: "tok1",
      }),
    );
  });
});
