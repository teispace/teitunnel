import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { ProjectEntry, ProjectStatus } from "@/lib/ipc/bindings";
import { ProjectsPage } from "./projects-page";

const core = (key: string, args: Record<string, string | number> = {}) => ({
  key: `core.${key}`,
  args,
});

const PATH = "/work/shop/teitunnel.yml";

let projects: ProjectEntry[];
let modified: number;
let calls: { cmd: string; args: Record<string, unknown> }[];

const status = (): ProjectStatus => ({
  path: PATH,
  name: "shop",
  diagnostics: [
    {
      line: 12,
      column: 1,
      severity: "warning",
      message: core("project.unknownKey", { key: "protection" }),
    },
  ],
  problem: null,
  modifiedAt: 1000,
  plan: {
    path: PATH,
    name: "shop",
    accountId: "a1",
    items: [
      {
        kind: "route",
        name: "shop.xyz.com",
        target: "3000",
        state: "missing",
        line: 3,
        note: null,
      },
      {
        kind: "route",
        name: "api.xyz.com",
        target: "4000",
        state: "applied",
        line: 5,
        note: null,
      },
      {
        kind: "share",
        name: "trycloudflare.com",
        target: "http://localhost:5173",
        state: "missing",
        line: 8,
        note: null,
      },
      {
        kind: "localDomain",
        name: "shop.localhost",
        target: "localhost:3000",
        state: "missing",
        line: 10,
        note: null,
      },
    ],
    routes: [
      {
        hostname: "shop.xyz.com",
        path: null,
        change: {
          type: "addRoute",
          route: {
            hostname: "shop.xyz.com",
            path: null,
            origin: "3000",
            access: null,
            options: null,
          },
        },
        tunnelId: null,
        plan: {
          steps: [
            {
              kind: "createRecord",
              description: core("plan.step.createRecord", {
                hostname: "shop.xyz.com",
                tunnel: "Mac",
              }),
              command: null,
            },
          ],
          warnings: [],
          requiresConfirmation: false,
          fingerprint: "route-fp",
        },
      },
    ],
    shares: [
      {
        origin: "http://localhost:5173",
        hostname: null,
        expiresAfter: null,
        hostHeader: { mode: "auto" },
        login: null,
        inspect: true,
      },
    ],
    snapshots: [],
    localDomains: [{ name: "shop.localhost", port: 3000, wildcard: false, exists: false }],
    requiresConfirmation: false,
    fingerprint: "project-fp",
  },
});

beforeEach(() => {
  projects = [{ path: PATH, name: "shop", addedAt: 1, appliedAt: null, createdRoutes: [] }];
  modified = 1000;
  calls = [];
  mockWindows("main");
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    switch (cmd) {
      case "projects_list":
        return projects;
      case "projects_status":
        return status();
      case "projects_modified":
        return modified;
      case "projects_choose_folder":
        return "/work/blog";
      case "projects_add":
        projects = [
          ...projects,
          {
            path: "/work/blog/teitunnel.yml",
            name: "blog",
            addedAt: 2,
            appliedAt: null,
            createdRoutes: [],
          },
        ];
        return projects[1];
      case "projects_apply":
        return {
          routes: { created: [], notes: [], failure: null },
          snapshots: [],
          shares: ["http://localhost:5173"],
          shareErrors: [],
        };
      default:
        return null;
    }
  });
});

function renderPage() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <ProjectsPage />
    </QueryClientProvider>,
  );
}

describe("ProjectsPage", () => {
  it("shows each declared item's state and the file's problems with their lines", async () => {
    renderPage();
    expect(await screen.findByText("shop.xyz.com")).toBeTruthy();
    expect(screen.getAllByText("Missing")).toHaveLength(3);
    expect(screen.getByText("Applied")).toBeTruthy();
    expect(screen.getByText("shop.localhost")).toBeTruthy();
    expect(screen.getByText("Random trycloudflare.com address")).toBeTruthy();
    expect(screen.getByText("Line 12, column 1")).toBeTruthy();
    expect(screen.getByText(/protection isn't known/)).toBeTruthy();
  });

  it("reviews the combined plan, then applies it by fingerprint", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Apply…" }));
    const sheet = await screen.findByRole("dialog");
    expect(within(sheet).getByText("Add DNS record shop.xyz.com → tunnel “Mac”")).toBeTruthy();
    expect(
      within(sheet).getByText("Share http://localhost:5173 at a random trycloudflare.com address"),
    ).toBeTruthy();
    expect(
      within(sheet).getByText("Serve https://shop.localhost from localhost:3000 on this computer"),
    ).toBeTruthy();
    expect(calls.some((c) => c.cmd === "projects_apply")).toBe(false);
    fireEvent.click(within(sheet).getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "projects_apply")).toBe(true));
    expect(calls.find((c) => c.cmd === "projects_apply")?.args).toEqual({
      path: PATH,
      fingerprint: "project-fp",
      confirmed: false,
    });
  });

  it("notices the file changing and offers to review it", async () => {
    renderPage();
    await screen.findByText("shop.xyz.com");
    expect(screen.queryByText("teitunnel.yml changed")).toBeNull();
    modified = 2000;
    expect(await screen.findByText("teitunnel.yml changed", {}, { timeout: 5000 })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Review Changes" }));
    expect(await screen.findByRole("dialog")).toBeTruthy();
  });

  it("opens another project from a folder", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Open Project…" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "projects_add")).toBe(true));
    expect(calls.find((c) => c.cmd === "projects_add")?.args).toEqual({ path: "/work/blog" });
  });
});
