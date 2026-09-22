import { render, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "./app";

describe("App", () => {
  it("renders the shell and the overview", async () => {
    const { findByText, getByRole } = render(<App />);
    expect(await findByText("Nothing running yet")).toBeTruthy();
    await waitFor(() => expect(getByRole("navigation")).toBeTruthy());
  });
});
