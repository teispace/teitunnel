import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { SubjectView, Thread_Serialize as Thread } from "@/lib/ipc/bindings";
import { CommentsPage, threadUrl } from "./comments-page";

let calls: { cmd: string; args: Record<string, unknown> }[];
let threads: Thread[];

const subjects: SubjectView[] = [
  {
    key: "snapshot:s1",
    kind: "snapshot",
    accountId: "a1",
    label: "Launch",
    url: "https://preview.xyz.com",
    open: 1,
    comments: 2,
    unread: 1,
    latestAt: Date.now() - 60_000,
  },
];

function thread(resolved: boolean): Thread {
  return {
    id: "c1",
    path: "/pricing",
    anchor: null,
    resolved,
    resolvedBy: resolved ? "Me" : null,
    resolvedAt: resolved ? Date.now() : null,
    createdAt: Date.now() - 120_000,
    comments: [
      {
        id: "c1",
        author: "Ana",
        email: null,
        verified: false,
        byOwner: false,
        body: "<b>Too small</b>",
        createdAt: Date.now() - 120_000,
      },
      {
        id: "c2",
        author: "boss@xyz.com",
        email: "boss@xyz.com",
        verified: true,
        byOwner: false,
        body: "Agreed",
        createdAt: Date.now() - 60_000,
      },
    ],
  };
}

beforeEach(() => {
  calls = [];
  threads = [thread(false)];
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "comments_subjects":
        return subjects;
      case "comments_threads":
        return threads;
      case "comments_reply":
        return threads[0];
      case "comments_resolve":
        threads = [thread(payload["resolved"] === true)];
        return threads[0];
      default:
        return null;
    }
  });
});

function renderPage(subject?: string) {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <CommentsPage subject={subject} />
    </QueryClientProvider>,
  );
}

describe("CommentsPage", () => {
  it("lists subjects with unread counts and shows threads as text", async () => {
    renderPage();
    expect(await screen.findByRole("option", { name: /Launch/ })).toBeTruthy();
    expect(screen.getByLabelText("1 unread comment")).toBeTruthy();
    const card = await screen.findByRole("article", { name: "Comments on /pricing" });
    // Text is rendered as text, never as markup.
    expect(within(card).getByText("<b>Too small</b>")).toBeTruthy();
    expect(within(card).getByText("Signed in")).toBeTruthy();
    expect(calls.some((c) => c.cmd === "comments_threads" && c.args["key"] === "snapshot:s1")).toBe(
      true,
    );
  });

  it("replies and resolves", async () => {
    renderPage("snapshot:s1");
    const card = await screen.findByRole("article", { name: "Comments on /pricing" });
    fireEvent.change(within(card).getByLabelText("Reply"), { target: { value: "Fixed now" } });
    fireEvent.click(within(card).getByRole("button", { name: "Reply" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "comments_reply")?.args).toEqual({
        key: "snapshot:s1",
        thread: "c1",
        body: "Fixed now",
      }),
    );
    fireEvent.click(within(card).getByRole("button", { name: "Resolve" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "comments_resolve")?.args["resolved"]).toBe(true),
    );
    // The thread moves to Resolved.
    expect(
      await screen.findByText("Nothing open. Resolved comments are under Resolved."),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: "Resolved (1)" }));
    expect(await screen.findByRole("button", { name: "Reopen" })).toBeTruthy();
  });

  it("links a thread to its spot on the page", () => {
    expect(threadUrl(subjects[0] as SubjectView, thread(false))).toBe(
      "https://preview.xyz.com/pricing#__teitunnel-comment=c1",
    );
  });

  it("explains what comments are when there are none", async () => {
    mockIPC((cmd) => (cmd === "comments_subjects" ? [] : null));
    renderPage();
    expect(await screen.findByText("No comments yet")).toBeTruthy();
  });
});
