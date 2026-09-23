import { render, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "./app";

describe("App", () => {
  it("renders the shell and the overview", async () => {
    const { findByText, getByRole } = render(<App />);
    // The whole app, with its lazily loaded route: allow for a busy test runner.
    expect(await findByText("Nothing running yet", {}, { timeout: 5_000 })).toBeTruthy();
    await waitFor(() => expect(getByRole("navigation")).toBeTruthy());
  });
});
