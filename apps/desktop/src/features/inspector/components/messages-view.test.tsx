import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { FrameRecord, StreamStats } from "@/lib/ipc/bindings";
import { filterMessages, messageRows, prettyMessage } from "../model";
import { MessagesView } from "./messages-view";

const frame = (
  direction: FrameRecord["direction"],
  preview: string | null,
  extra: Partial<FrameRecord> = {},
): FrameRecord => ({
  atUs: 1_000,
  direction,
  opcode: "text",
  fin: true,
  masked: direction === "clientToServer",
  compressed: preview === null,
  size: preview?.length ?? 40,
  preview,
  truncated: false,
  closeCode: null,
  closeReason: null,
  ...extra,
});

const socket: StreamStats = {
  client: { count: 4, bytes: 120 },
  server: { count: 4, bytes: 300 },
  previews: [],
  frames: [
    frame("clientToServer", '{"type":"subscribe","channel":"orders"}'),
    frame("serverToClient", '{"type":"ack"}'),
    frame("serverToClient", '{"type":"order","id":7}'),
    frame("clientToServer", "ping"),
    frame("serverToClient", null),
    frame("clientToServer", null, { opcode: "close", closeCode: 1000, closeReason: "bye" }),
  ],
  framesDropped: 0,
  closed: true,
} as unknown as StreamStats;

describe("messages", () => {
  it("rows come from frames, filtered by direction and text", () => {
    const rows = messageRows(socket);
    expect(rows).toHaveLength(6);
    expect(rows[5]?.text).toBe("1000 bye");
    expect(rows[4]?.text).toBeNull();
    expect(filterMessages(rows, "serverToClient", "")).toHaveLength(3);
    expect(filterMessages(rows, "all", "ORDER")).toHaveLength(2);
    expect(filterMessages(rows, "clientToServer", "order")).toHaveLength(1);
    expect(prettyMessage('{"a":1}')).toBe('{\n  "a": 1\n}');
    expect(prettyMessage("{not json")).toBe("{not json");
    expect(prettyMessage("plain")).toBe("plain");
  });

  it("filters and opens a frame to read it whole", () => {
    render(<MessagesView stream={socket} />);
    const list = screen.getByRole("list", { name: "WebSocket frames" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(6);

    fireEvent.click(screen.getByRole("radio", { name: "Service" }));
    expect(within(list).getAllByRole("listitem")).toHaveLength(3);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search Messages" }), {
      target: { value: "order" },
    });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(1);

    const row = within(rows[0] as HTMLElement).getByRole("button");
    expect(row.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(row);
    expect(row.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText(/"id": 7/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /Copy/ })).toBeTruthy();

    fireEvent.change(screen.getByRole("searchbox", { name: "Search Messages" }), {
      target: { value: "nothing like this" },
    });
    expect(screen.getByText("No messages match.")).toBeTruthy();
  });
});
