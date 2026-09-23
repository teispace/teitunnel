import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { LocalService } from "@/lib/ipc/bindings";
import { ServicePicker } from "./service-picker";

function service(
  port: number,
  process: string,
  kind: LocalService["kind"],
  project: string,
): LocalService {
  return {
    port,
    process,
    kind,
    project,
    pid: port,
    allInterfaces: false,
    origin: `http://localhost:${port}`,
  };
}

const services: LocalService[] = [
  service(5173, "node", "vite", "web"),
  service(8000, "python3", "django", "api"),
];

beforeEach(() => {
  mockWindows("main");
  mockIPC((cmd) => (cmd === "services_list" ? services : null));
});

function Harness() {
  const [value, setValue] = useState("");
  return (
    <QueryClientProvider client={createQueryClient()}>
      <ServicePicker value={value} onChange={setValue} />
      <output>{value}</output>
      <button type="button">elsewhere</button>
    </QueryClientProvider>
  );
}

const field = () => screen.getByRole("combobox", { name: "Port or address" });
/** A real click on the field: press, focus, release. */
async function press(element: HTMLElement) {
  fireEvent.pointerDown(element);
  fireEvent.mouseDown(element);
  act(() => element.focus());
  // Radix starts watching for outside presses a tick after the list opens.
  await act(() => new Promise((resolve) => setTimeout(resolve, 20)));
  fireEvent.pointerDown(element);
  fireEvent.pointerUp(element);
  fireEvent.click(element);
}

describe("ServicePicker", () => {
  it("keeps the list open when the field is clicked, every time", async () => {
    render(<Harness />);
    await press(field());
    expect(await screen.findByRole("listbox")).toBeTruthy();
    // Leave and come back: the list must open again and stay open.
    act(() => field().blur());
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    await press(field());
    await act(() => new Promise((resolve) => setTimeout(resolve, 50)));
    expect(screen.getByRole("listbox")).toBeTruthy();
  });

  it("picks a service, and closes on Escape", async () => {
    render(<Harness />);
    await press(field());
    fireEvent.mouseDown(await screen.findByRole("option", { name: /django/i }));
    expect(screen.getByText("8000", { selector: "output" })).toBeTruthy();
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    fireEvent.keyDown(field(), { key: "ArrowDown" });
    expect(await screen.findByRole("listbox")).toBeTruthy();
    fireEvent.keyDown(field(), { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
  });
});
