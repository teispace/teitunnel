import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { inspectorMock, mockRows } from "@/dev/mock-inspector";
import type { ExchangeRow, LiveBatch } from "@/lib/ipc/bindings";
import { InspectorPage } from "./inspector-page";

let calls: { cmd: string; args: Record<string, unknown> }[];
let channel: { onmessage: (batch: LiveBatch) => void } | null;

beforeEach(() => {
  calls = [];
  channel = null;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    if (cmd === "inspect_subscribe") {
      channel = payload["onBatch"] as typeof channel;
      return 7;
    }
    if (cmd === "doctor_run") return [];
    if (cmd === "settings_get") return { ignoredIssues: [] };
    return inspectorMock(cmd, payload) ?? null;
  });
});

function renderPage(props: { tap?: string } = {}) {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>
        <InspectorPage {...props} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const list = () => screen.findByRole("listbox", { name: "Requests" });
const options = () =>
  within(screen.getByRole("listbox", { name: "Requests" })).queryAllByRole("option");
const called = (cmd: string) => calls.filter((c) => c.cmd === cmd);

const send = (exchanges: ExchangeRow[], extra: Partial<LiveBatch> = {}) =>
  act(() =>
    channel?.onmessage({ exchanges, cleared: [], tapsChanged: false, lagged: false, ...extra }),
  );

describe("InspectorPage", () => {
  it("lists captured requests, adds live batches at the top, and unsubscribes on unmount", async () => {
    const view = renderPage();
    await list();
    await waitFor(() => expect(options().length).toBeGreaterThan(10));
    expect(called("inspect_exchanges")[0]?.args["query"]).toMatchObject({ tap: null, limit: 1000 });
    await waitFor(() => expect(channel).not.toBeNull());
    const fresh: ExchangeRow = {
      ...(mockRows()[0] as ExchangeRow),
      id: "ex-live",
      seq: 99,
      method: "PUT",
      path: "/api/live",
    };
    send([fresh]);
    await waitFor(() => expect(options()[0]?.textContent).toContain("/api/live"));
    // A later batch updates it in place.
    send([{ ...fresh, status: 503 }]);
    await waitFor(() => expect(options()[0]?.textContent).toContain("503"));
    view.unmount();
    await waitFor(() => expect(called("inspect_unsubscribe")[0]?.args["id"]).toBe(7));
  });

  it("holds live requests back while paused", async () => {
    renderPage();
    await list();
    await waitFor(() => expect(channel).not.toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    send([{ ...(mockRows()[0] as ExchangeRow), id: "ex-held", path: "/held" }]);
    expect(await screen.findByText("Paused · 1 new request")).toBeTruthy();
    expect(screen.queryByText("/held")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    expect(await screen.findByText("/held")).toBeTruthy();
  });

  it("filters by status class, method and path, and searches in the inspector", async () => {
    renderPage();
    await list();
    await waitFor(() => expect(options().length).toBeGreaterThan(10));
    fireEvent.click(screen.getByRole("radio", { name: "5xx" }));
    await waitFor(() => expect(options().every((o) => o.textContent?.includes("500"))).toBe(true));
    expect(options().length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("radio", { name: "All" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Filter by path or host" }), {
      target: { value: "webhooks" },
    });
    await waitFor(() =>
      expect(options().every((o) => o.textContent?.includes("/webhooks/"))).toBe(true),
    );
    fireEvent.change(screen.getByRole("searchbox", { name: "Search requests" }), {
      target: { value: "stripe" },
    });
    await waitFor(() =>
      expect(
        called("inspect_exchanges").some(
          (c) => (c.args["query"] as { text?: string }).text === "stripe",
        ),
      ).toBe(true),
    );
  });

  it("shows a request masked, reveals it on request, and never keeps the secrets", async () => {
    renderPage();
    await list();
    await waitFor(() => expect(options().length).toBeGreaterThan(10));
    fireEvent.mouseDown(options()[0] as HTMLElement, { button: 0 });
    const detail = await screen.findByRole("region", { name: "Request details" });
    expect((await within(detail).findAllByText("[redacted]")).length).toBeGreaterThan(0);
    expect(called("inspect_exchange").every((c) => c.args["reveal"] === false)).toBe(true);

    fireEvent.click(within(detail).getByRole("button", { name: "Show Secrets" }));
    expect(await within(detail).findByText(/session=eyJ/)).toBeTruthy();
    expect(called("inspect_exchange").some((c) => c.args["reveal"] === true)).toBe(true);

    fireEvent.click(within(detail).getByRole("button", { name: "Hide Secrets" }));
    await waitFor(() => expect(within(detail).queryByText(/session=eyJ/)).toBeNull());
  });

  it("edits and replays a request, sending only what changed", async () => {
    renderPage();
    await list();
    await waitFor(() => expect(options().length).toBeGreaterThan(10));
    fireEvent.mouseDown(options()[0] as HTMLElement, { button: 0 });
    const detail = await screen.findByRole("region", { name: "Request details" });
    fireEvent.click(await within(detail).findByRole("button", { name: /Edit and Replay/ }));
    const sheet = await screen.findByRole("dialog", { name: "Edit and Replay" });
    const headers = within(sheet).getByRole("textbox", { name: "Headers" }) as HTMLTextAreaElement;
    fireEvent.change(headers, { target: { value: `${headers.value}\nx-debug: 1` } });
    fireEvent.change(within(sheet).getByRole("textbox", { name: "Send" }), {
      target: { value: "3" },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Send 3 Times" }));
    await waitFor(() => expect(called("inspect_replay")).toHaveLength(1));
    expect(called("inspect_replay")[0]?.args["input"]).toEqual({
      setHeaders: [["x-debug", "1"]],
      times: 3,
    });
  });

  it("exports redacted by default, and unredacted only when Redact is unticked", async () => {
    renderPage();
    await list();
    await waitFor(() => expect(options().length).toBeGreaterThan(10));
    fireEvent.mouseDown(options()[0] as HTMLElement, { button: 0 });
    fireEvent.click(await screen.findByRole("button", { name: "Export Requests…" }));
    const sheet = await screen.findByRole("dialog", { name: "Export Requests" });
    expect(await within(sheet).findByText(/curl 'https:/)).toBeTruthy();
    expect(called("inspect_export")[0]?.args).toMatchObject({ format: "curl", redact: true });
    fireEvent.click(within(sheet).getByRole("checkbox", { name: "Redact" }));
    await waitFor(() =>
      expect(called("inspect_export").some((c) => c.args["redact"] === false)).toBe(true),
    );
    expect(within(sheet).getByText(/includes credentials/)).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("button", { name: "Save to Downloads" }));
    await waitFor(() => expect(called("inspect_export_save")[0]?.args["redact"]).toBe(false));
  });

  it("compares two requests picked with ⌘-click", async () => {
    renderPage();
    await list();
    await waitFor(() => expect(options().length).toBeGreaterThan(10));
    fireEvent.mouseDown(options()[0] as HTMLElement, { button: 0 });
    fireEvent.mouseDown(options()[1] as HTMLElement, { button: 0, metaKey: true });
    fireEvent.click(screen.getByRole("button", { name: "Compare Two Requests" }));
    const sheet = await screen.findByRole("dialog", { name: "Compare Requests" });
    expect(await within(sheet).findByText("Request headers")).toBeTruthy();
  });

  it("shows one tap's requests", async () => {
    renderPage({ tap: "rt-docs" });
    await list();
    await waitFor(() => expect(options().length).toBeGreaterThan(0));
    expect(called("inspect_exchanges")[0]?.args["query"]).toMatchObject({ tap: "rt-docs" });
    expect(options().every((o) => !o.textContent?.includes("/webhooks"))).toBe(true);
  });
});
