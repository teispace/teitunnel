import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { Account } from "@/lib/ipc/bindings";
import { AccountsPane } from "./components/accounts-pane";

let accounts: Account[];
const sentTokens: string[] = [];

beforeEach(() => {
  accounts = [];
  sentTokens.length = 0;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "accounts_list":
        return accounts;
      case "accounts_detect_cert":
        return false;
      case "accounts_add_token": {
        const token = String(payload["token"]);
        sentTokens.push(token);
        if (token !== "good-token") {
          throw {
            code: "invalidInput",
            message: "Cloudflare didn't accept this token.",
            hint: null,
            field: "credential",
          };
        }
        accounts = [{ id: "a1", name: "Personal", credential: "apiToken", limitedZone: null }];
        return accounts;
      }
      case "accounts_remove":
        accounts = accounts.filter((a) => a.id !== payload["id"]);
        return null;
      case "accounts_capabilities":
        return {
          zonesRead: "yes",
          tunnelsRead: "yes",
          tunnelsEdit: "no",
          accessEdit: "no",
          zones: [],
        };
      default:
        return null;
    }
  });
});

function renderPane() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <AccountsPane />
    </QueryClientProvider>,
  );
}

describe("Accounts", () => {
  it("rejects a bad token inline, then connects with a good one", async () => {
    renderPane();
    fireEvent.click(await screen.findByRole("button", { name: /Connect an Account/ }));
    const sheet = await screen.findByRole("dialog", { name: "Connect Cloudflare" });
    const field = within(sheet).getByLabelText("API token");
    expect(field.getAttribute("type")).toBe("password");

    fireEvent.change(field, { target: { value: "bad-token" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Connect" }));
    expect(await within(sheet).findByText("Cloudflare didn't accept this token.")).toBeTruthy();

    fireEvent.change(field, { target: { value: "good-token" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Connect" }));
    expect(await screen.findByText("Personal")).toBeTruthy();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(sentTokens).toEqual(["bad-token", "good-token"]);
  });

  it("disconnects after confirmation", async () => {
    accounts = [{ id: "a1", name: "Personal", credential: "certPem", limitedZone: "z1" }];
    renderPane();
    expect(await screen.findByText("cloudflared login · one domain only")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    const dialog = await screen.findByRole("dialog", { name: "Disconnect Personal?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Disconnect" }));
    expect(await screen.findByText("No accounts connected")).toBeTruthy();
  });
});
