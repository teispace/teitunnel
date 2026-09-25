import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { PlanView, SnapshotView } from "@/lib/ipc/bindings";
import { SnapshotsPage } from "./snapshots-page";

const core = (key: string, args: Record<string, string | number> = {}) => ({
  key: `core.${key}`,
  args,
});

let snapshots: SnapshotView[];
let calls: { cmd: string; args: Record<string, unknown> }[];

const plan: PlanView = {
  steps: [
    {
      kind: "snapshot",
      description: core("snapshot.step.upload", { count: 2, total: 2, size: "1.2 KB" }),
      command: null,
    },
    {
      kind: "snapshot",
      description: core("snapshot.step.createWorker", { script: "teitunnel-site" }),
      command: null,
    },
    {
      kind: "snapshotAddress",
      description: core("snapshot.step.enableWorkersDev", {
        address: "teitunnel-site.acme.workers.dev",
      }),
      command: null,
    },
  ],
  warnings: [],
  requiresConfirmation: false,
  fingerprint: "fp-1",
};

const launch: SnapshotView = {
  id: "s1",
  accountId: "a1",
  name: "Launch",
  url: "https://preview.xyz.com",
  hostname: "preview.xyz.com",
  workersDev: false,
  script: "teitunnel-launch",
  source: { type: "folder", path: "/work/launch/dist" },
  spa: false,
  password: true,
  access: null,
  expiresAt: null,
  createdAt: 1,
  updatedAt: Date.now(),
  liveVersion: 2,
  versions: 2,
  files: 3,
  bytes: 2_500,
  comments: false,
};

beforeEach(() => {
  snapshots = [];
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
          workersEdit: "yes",
          zones: [],
        };
      case "domains_list":
        return [];
      case "snapshots_list":
        return snapshots;
      case "snapshots_versions":
        return [
          {
            number: 2,
            createdAt: Date.now(),
            files: 3,
            bytes: 2_500,
            live: true,
            spa: false,
            password: true,
          },
          {
            number: 1,
            createdAt: Date.now() - 60_000,
            files: 2,
            bytes: 1_200,
            live: false,
            spa: false,
            password: false,
          },
        ];
      case "snapshots_choose_folder":
        return "/work/site";
      case "snapshots_prepare_folder":
        return {
          id: "prep-1",
          source: { type: "folder", path: "/work/site" },
          suggestedName: "site",
          files: 2,
          bytes: 1_200,
          skipped: [{ path: ".env", reason: "secret" }],
          singlePage: true,
          crawl: null,
        };
      case "snapshots_preview":
        return plan;
      case "snapshots_apply":
        return { type: "applied", tunnelId: null, verify: [], connectorError: null };
      default:
        return null;
    }
  });
});

function renderPage() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <SnapshotsPage />
    </QueryClientProvider>,
  );
}

describe("SnapshotsPage", () => {
  it("publishes a folder: choose it, review the plan, then apply it", async () => {
    renderPage();
    const [publish] = await screen.findAllByRole("button", { name: "Publish a Snapshot" });
    fireEvent.click(publish as HTMLElement);
    fireEvent.click(await screen.findByRole("button", { name: /Choose Folder/ }));
    expect(await screen.findByText("/work/site")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    // Details: the name comes from the folder; one HTML page suggests a single-page app.
    const name = await screen.findByLabelText("Name");
    expect((name as HTMLInputElement).value).toBe("site");
    expect(screen.getByRole("switch", { name: "Single-page app" }).getAttribute("data-state")).toBe(
      "checked",
    );
    fireEvent.click(screen.getByRole("button", { name: "Review" }));

    expect(await screen.findByText(/Create the Worker teitunnel-site/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Publish" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "snapshots_apply")).toBe(true));
    const apply = calls.find((c) => c.cmd === "snapshots_apply");
    expect(apply?.args["fingerprint"]).toBe("fp-1");
    expect(apply?.args["change"]).toEqual({
      type: "publish",
      prepared: "prep-1",
      name: "site",
      address: { type: "workersDev" },
      options: {
        spa: true,
        password: { type: "remove" },
        access: null,
        expiresInDays: null,
        comments: false,
      },
    });
  });

  it("shows a Snapshot's details and rolls back to an earlier version", async () => {
    snapshots = [launch];
    renderPage();
    const inspector = await screen.findByRole("complementary");
    expect(await within(inspector).findByText("Anyone with the password")).toBeTruthy();
    expect(within(inspector).getByText("https://preview.xyz.com")).toBeTruthy();
    expect(await within(inspector).findByText("Version 1")).toBeTruthy();

    fireEvent.click(within(inspector).getByRole("button", { name: /Roll Back/ }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Make Live" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "snapshots_apply")).toBe(true));
    expect(calls.find((c) => c.cmd === "snapshots_preview")?.args["change"]).toEqual({
      type: "rollback",
      snapshot: "s1",
      version: 1,
    });
  });

  it("deletes a Snapshot after confirming", async () => {
    snapshots = [launch];
    renderPage();
    const inspector = await screen.findByRole("complementary");
    fireEvent.click(within(inspector).getByRole("button", { name: /^Delete$/ }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Delete Launch?")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete Snapshot" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "snapshots_apply")?.args["change"]).toEqual({
        type: "delete",
        snapshot: "s1",
      }),
    );
  });
});
