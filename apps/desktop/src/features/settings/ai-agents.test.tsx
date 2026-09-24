import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { AiAgentsView } from "@/lib/ipc/bindings";
import { AiAgents } from "./ai-agents";
import { ApiDescription } from "./api-description";

let view: AiAgentsView;
let calls: string[];

beforeEach(() => {
  calls = [];
  view = { agents: [], approvals: [] };
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "ai_agents") return view;
    if (cmd === "inspect_openapi_save") {
      return {
        path: "/Users/me/Downloads/teitunnel-openapi.json",
        summary: { requests: 12, skipped: 3, paths: 4, operations: 6, hosts: ["api.xyz.com"] },
      };
    }
    return null;
  });
});

function renderWith(node: React.ReactNode) {
  return render(<QueryClientProvider client={createQueryClient()}>{node}</QueryClientProvider>);
}

describe("connected agents", () => {
  it("says when none is connected", async () => {
    renderWith(<AiAgents />);
    expect(await screen.findByText("No agent is connected.")).toBeTruthy();
  });

  it("lists agents and the changes waiting for an answer", async () => {
    view = {
      agents: [
        { name: "claude-code", version: "2.1", mode: "ask", connectedAt: Date.now() },
        { name: "cursor", version: null, mode: "full", connectedAt: Date.now() },
      ],
      approvals: [{ agent: "claude-code", title: "Add api.xyz.com", askedAt: Date.now() }],
    };
    renderWith(<AiAgents />);
    expect(await screen.findByText("claude-code 2.1")).toBeTruthy();
    expect(screen.getByText("Asks first")).toBeTruthy();
    expect(screen.getByText("Full access")).toBeTruthy();
    expect(screen.getByText("Add api.xyz.com")).toBeTruthy();
    expect(screen.getByText("claude-code is waiting for your answer")).toBeTruthy();
  });
});

describe("the API description", () => {
  it("saves an OpenAPI file to Downloads", async () => {
    renderWith(<ApiDescription />);
    fireEvent.click(screen.getByRole("button", { name: "Save to Downloads" }));
    await waitFor(() => expect(calls).toContain("inspect_openapi_save"));
  });
});
