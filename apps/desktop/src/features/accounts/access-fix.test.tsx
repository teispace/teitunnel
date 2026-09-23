import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { Account, Grant } from "@/lib/ipc/bindings";
import { AccessFix } from "./components/access-fix";

let account: Account;
let accessEdit: Grant;
let probes: number;
const opened: string[] = [];
const sentTokens: string[] = [];

beforeEach(() => {
  account = { id: "a1", name: "Personal", credential: "apiToken", limitedZone: null };
  accessEdit = "no";
  probes = 0;
  opened.length = 0;
  sentTokens.length = 0;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "accounts_list":
        return [account];
      case "accounts_capabilities":
        probes += 1;
        return { zonesRead: "yes", tunnelsRead: "yes", tunnelsEdit: "yes", accessEdit, zones: [] };
      case "accounts_open_token_page":
        opened.push(String(payload["page"]));
        return null;
      case "accounts_add_token":
        sentTokens.push(String(payload["token"]));
        account = { ...account, credential: "apiToken" };
        accessEdit = "yes";
        return [account];
      default:
        return null;
    }
  });
});

function renderFix(onReady = vi.fn()) {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <AccessFix accountId="a1" onReady={onReady} />
    </QueryClientProvider>,
  );
  return onReady;
}

describe("AccessFix", () => {
  it("walks through editing the token, then notices the fix on return", async () => {
    const onReady = renderFix();
    expect(await screen.findByText("Logins need two more permissions")).toBeTruthy();
    expect(screen.getByText(/Access: Apps and Policies · Edit/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /Open API Tokens/ }));
    await waitFor(() => expect(opened).toEqual(["edit"]));

    // Still missing after a manual check: say so.
    fireEvent.click(screen.getByRole("button", { name: "Check Again" }));
    expect(await screen.findByText(/still can't manage logins/)).toBeTruthy();

    // The permission was added in the browser; coming back re-checks by itself.
    accessEdit = "yes";
    const before = probes;
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await waitFor(() => expect(onReady).toHaveBeenCalledTimes(1));
    expect(probes).toBeGreaterThan(before);
    expect(screen.queryByText("Logins need two more permissions")).toBeNull();
  });

  it("offers a new token for credentials that can't gain the permission", async () => {
    account = { id: "a1", name: "Personal", credential: "certPem", limitedZone: "z1" };
    const onReady = renderFix();
    expect(await screen.findByText(/connected with a login that can't manage Access/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Open API Tokens/ })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Create Token/ }));
    await waitFor(() => expect(opened).toEqual(["create"]));
    const field = screen.getByLabelText("API token");
    fireEvent.change(field, { target: { value: "new-token" } });
    // Enter connects (it's inside the route form, which must not submit).
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(onReady).toHaveBeenCalledTimes(1));
    expect(sentTokens).toEqual(["new-token"]);
  });

  it("renders nothing when logins are allowed", async () => {
    accessEdit = "yes";
    const onReady = renderFix();
    await waitFor(() => expect(probes).toBe(1));
    expect(screen.queryByText("Logins need two more permissions")).toBeNull();
    expect(onReady).not.toHaveBeenCalled();
  });
});
