import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { BackupPreview } from "@/lib/ipc/bindings";
import { MoveComputer } from "./move-computer";

let calls: { cmd: string; args: Record<string, unknown> }[];
let wrongPassphrase: boolean;

const preview: BackupPreview = {
  id: "1-Ada's Mac",
  summary: {
    createdAt: 1,
    appVersion: "0.2.0",
    machine: "Ada's Mac",
    accounts: [{ id: "a1", name: "Acme" }],
    projects: ["shop"],
    sections: [
      { section: "settings", count: 6, existing: 2 },
      { section: "local_tunnels", count: 1, existing: 1 },
    ],
    overwrites: true,
  },
};

beforeEach(() => {
  calls = [];
  wrongPassphrase = false;
  mockWindows("settings");
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    switch (cmd) {
      case "backup_choose_save":
        return "/Users/ada/Teitunnel Setup.teitunnel-backup";
      case "backup_choose_open":
        return "/Volumes/usb/Teitunnel Setup.teitunnel-backup";
      case "backup_inspect":
        if (wrongPassphrase) {
          throw {
            code: "invalidInput",
            message: { key: "core.backup.wrongPassphrase", args: {} },
            hint: null,
            field: "passphrase",
          };
        }
        return preview;
      default:
        return null;
    }
  });
});

function renderIt() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <MoveComputer />
    </QueryClientProvider>,
  );
}

describe("Move to another computer", () => {
  it("exports with a passphrase typed twice", async () => {
    renderIt();
    fireEvent.click(screen.getByRole("button", { name: "Export…" }));
    const sheet = await screen.findByRole("dialog");
    const save = within(sheet).getByRole("button", { name: "Save Backup" });
    fireEvent.change(within(sheet).getByLabelText("Passphrase"), {
      target: { value: "correct horse battery" },
    });
    fireEvent.change(within(sheet).getByLabelText("Passphrase again"), {
      target: { value: "correct horse batter" },
    });
    expect(within(sheet).getByText("The passphrases don't match.")).toBeTruthy();
    expect(save.hasAttribute("disabled")).toBe(true);
    fireEvent.change(within(sheet).getByLabelText("Passphrase again"), {
      target: { value: "correct horse battery" },
    });
    fireEvent.click(save);
    await waitFor(() => expect(calls.some((c) => c.cmd === "backup_create")).toBe(true));
    expect(calls.find((c) => c.cmd === "backup_create")?.args).toEqual({
      path: "/Users/ada/Teitunnel Setup.teitunnel-backup",
      passphrase: "correct horse battery",
    });
  });

  it("shows what a backup brings and replaces before restoring it", async () => {
    renderIt();
    fireEvent.click(screen.getByRole("button", { name: "Import…" }));
    const sheet = await screen.findByRole("dialog");
    fireEvent.change(within(sheet).getByLabelText("Passphrase"), {
      target: { value: "correct horse battery" },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Read Backup" }));
    expect(await within(sheet).findByText("Made on Ada's Mac with Teitunnel 0.2.0.")).toBeTruthy();
    expect(
      within(sheet).getByText(/Connect these accounts again after restoring: Acme/),
    ).toBeTruthy();
    expect(within(sheet).getByText("1 · replaces 1 here")).toBeTruthy();
    expect(calls.some((c) => c.cmd === "backup_restore")).toBe(false);
    fireEvent.click(within(sheet).getByRole("button", { name: "Replace and Restore" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "backup_restore")).toBe(true));
    expect(calls.find((c) => c.cmd === "backup_restore")?.args).toEqual({ id: "1-Ada's Mac" });
  });

  it("says when the passphrase is wrong", async () => {
    wrongPassphrase = true;
    renderIt();
    fireEvent.click(screen.getByRole("button", { name: "Import…" }));
    const sheet = await screen.findByRole("dialog");
    fireEvent.change(within(sheet).getByLabelText("Passphrase"), { target: { value: "nope" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Read Backup" }));
    expect(
      await within(sheet).findByText(
        "The passphrase is wrong, or the file was changed or damaged.",
      ),
    ).toBeTruthy();
  });
});
