import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Verification } from "@/lib/ipc/bindings";
import { CheckNotes } from "./check-notes";

const works: Verification = {
  hostname: "shop.teispace.com",
  status: 200,
  failure: null,
  message: null,
  protected: false,
  eventStream: false,
  links: null,
  transient: false,
};

describe("CheckNotes", () => {
  it("says nothing about a page that works", () => {
    const { container } = render(<CheckNotes check={works} />);
    expect(container.textContent).toBe("");
  });

  it("explains a page linking to this computer, with the framework's fix", () => {
    const onCheck = vi.fn();
    render(
      <CheckNotes
        check={{
          ...works,
          links: {
            kind: "local",
            example: "http://[::1]:5173/@vite/client",
            message: {
              key: "core.devServer.links.local",
              args: { example: "http://[::1]:5173/@vite/client" },
            },
            fix: {
              key: "core.devServer.links.localLaravel",
              args: { url: "https://shop.teispace.com" },
            },
          },
        }}
        onCheck={onCheck}
      />,
    );
    expect(
      screen.getByText(
        "The page loads http://[::1]:5173/@vite/client from this computer, which visitors can't reach.",
      ),
    ).toBeTruthy();
    expect(screen.getByText(/APP_URL=https:\/\/shop\.teispace\.com/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Check Again" }));
    expect(onCheck).toHaveBeenCalled();
  });
});
