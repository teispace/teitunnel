import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { AiAgentsView } from "@/lib/ipc/bindings";
import { AgentsTab } from "./agents-tab";

let view: AiAgentsView;

beforeEach(() => {
  view = { agents: [], approvals: [] };
  mockIPC((cmd) => (cmd === "ai_agents" ? view : null));
});

function renderTab() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <AgentsTab />
    </QueryClientProvider>,
  );
}

describe("AgentsTab", () => {
  it("explains agents when none is connected", async () => {
    renderTab();
    expect(await screen.findByText(/No AI agent is connected right now/)).toBeTruthy();
    expect(screen.queryByText("Waiting for Your Answer")).toBeNull();
  });

  it("lists agents with what each may do, and the changes waiting for an answer", async () => {
    view = {
      agents: [
        { name: "claude-code", version: "2.1", mode: "ask", connectedAt: Date.now() },
        { name: "cursor", version: null, mode: "full", connectedAt: Date.now() },
      ],
      approvals: [{ agent: "claude-code", title: "Add api.xyz.com", askedAt: Date.now() }],
    };
    renderTab();
    expect(await screen.findByText("claude-code 2.1")).toBeTruthy();
    expect(screen.getByText("Asks first")).toBeTruthy();
    expect(screen.getByText("Full access")).toBeTruthy();
    expect(screen.getByText("Add api.xyz.com")).toBeTruthy();
    expect(
      screen.getByText("claude-code is waiting for you to answer in the dialog."),
    ).toBeTruthy();
  });
});
