import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "./confirm-dialog";

function deferred() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderDialog(onConfirm: () => Promise<unknown>) {
  render(
    <ConfirmDialog
      trigger={<Button>Disconnect…</Button>}
      title="Disconnect Acme?"
      confirmLabel="Disconnect"
      onConfirm={onConfirm}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Disconnect…" }));
}

describe("ConfirmDialog", () => {
  it("stays open and busy until the work is done, then closes", async () => {
    const work = deferred();
    renderDialog(() => work.promise);
    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));

    const confirm = screen.getByRole("button", { name: "Disconnect" });
    expect(confirm.getAttribute("aria-busy")).toBe("true");
    expect(screen.getByRole("button", { name: "Cancel" }).hasAttribute("disabled")).toBe(true);
    // Escape can't abandon it half way.
    fireEvent.keyDown(confirm, { key: "Escape" });
    expect(screen.getByRole("dialog")).toBeTruthy();

    await act(async () => work.resolve());
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows why it failed and lets the user try again", async () => {
    let attempt = 0;
    renderDialog(() =>
      ++attempt === 1 ? Promise.reject(new Error("Cloudflare didn't answer")) : Promise.resolve(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Cloudflare didn't answer");
    expect(screen.getByRole("dialog")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
