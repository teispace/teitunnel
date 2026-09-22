import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CopyField } from "./copy-field";

describe("CopyField", () => {
  it("copies the value and confirms", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<CopyField label="URL" value="https://a.example.com" />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy URL" }));
    });
    expect(writeText).toHaveBeenCalledWith("https://a.example.com");
    expect(screen.getByRole("button", { name: "URL copied" })).toBeTruthy();
  });
});
