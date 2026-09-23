import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LogLine } from "@/lib/ipc/bindings";
import { LogViewer } from "./log-viewer";

const line = (level: string, message: string, error: string | null = null): LogLine => ({
  time: null,
  level,
  message,
  error,
});

const lines = [
  line("info", "Starting tunnel"),
  line("warn", "Retrying connection to the edge"),
  line(
    "error",
    "Unable to reach the origin service",
    "dial tcp 127.0.0.1:3000: connection refused",
  ),
  line("info", "Registered tunnel connection location=ams01"),
];

describe("LogViewer", () => {
  it("filters by level and highlights search matches", () => {
    render(<LogViewer lines={lines} empty="Nothing yet." />);
    const log = screen.getByRole("log");
    expect(within(log).getAllByRole("listitem")).toHaveLength(4);

    fireEvent.click(screen.getByRole("radio", { name: "Errors" }));
    expect(within(log).getAllByRole("listitem")).toHaveLength(1);
    fireEvent.click(screen.getByRole("radio", { name: "Warnings" }));
    expect(within(log).getAllByRole("listitem")).toHaveLength(2);

    fireEvent.click(screen.getByRole("radio", { name: "All" }));
    fireEvent.change(screen.getByRole("searchbox", { name: "Search the log" }), {
      target: { value: "REFUSED" },
    });
    const items = within(log).getAllByRole("listitem");
    expect(items).toHaveLength(1);
    expect(items[0]?.querySelector("mark")?.textContent).toBe("refused");

    fireEvent.change(screen.getByRole("searchbox", { name: "Search the log" }), {
      target: { value: "nope" },
    });
    expect(within(log).getByText("No lines match.")).toBeTruthy();
  });

  it("keeps showing a paused snapshot", () => {
    const { rerender } = render(<LogViewer lines={lines.slice(0, 2)} empty="Nothing yet." />);
    fireEvent.click(screen.getByRole("button", { name: "Pause the log" }));
    rerender(<LogViewer lines={lines} empty="Nothing yet." />);
    expect(within(screen.getByRole("log")).getAllByRole("listitem")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Resume the log" }));
    expect(within(screen.getByRole("log")).getAllByRole("listitem")).toHaveLength(4);
  });

  it("renders only the rows in view, however many lines there are", () => {
    const many = Array.from({ length: 5_000 }, (_, i) => line("info", `request ${i}`));
    render(<LogViewer lines={many} empty="Nothing yet." />);
    const rendered = within(screen.getByRole("log")).getAllByRole("listitem");
    expect(rendered.length).toBeGreaterThan(0);
    expect(rendered.length).toBeLessThan(200);
  });

  it("saves the visible lines as text", () => {
    const onSave = vi.fn();
    render(<LogViewer lines={lines} empty="Nothing yet." onSave={onSave} />);
    fireEvent.click(screen.getByRole("radio", { name: "Errors" }));
    fireEvent.click(screen.getByRole("button", { name: "Save the visible lines to a file" }));
    expect(onSave).toHaveBeenCalledWith([
      "Unable to reach the origin service dial tcp 127.0.0.1:3000: connection refused",
    ]);
  });
});
