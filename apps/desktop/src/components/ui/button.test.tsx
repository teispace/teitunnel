import { render, screen } from "@testing-library/react";
import { Plus } from "lucide-react";
import { describe, expect, it } from "vitest";
import { Button } from "./button";
import { IconButton } from "./icon-button";

describe("Button", () => {
  it("is busy and disabled while pending, keeping its name", () => {
    render(
      <Button pending>
        <Plus /> Add
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Add" });
    expect(button.getAttribute("aria-busy")).toBe("true");
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(button.querySelector("[data-spinner]")).toBeTruthy();
  });

  it("is an ordinary button otherwise", () => {
    render(<Button>Add</Button>);
    const button = screen.getByRole("button", { name: "Add" });
    expect(button.hasAttribute("aria-busy")).toBe(false);
    expect(button.hasAttribute("disabled")).toBe(false);
    expect(button.querySelector("[data-spinner]")).toBeNull();
  });
});

describe("IconButton", () => {
  it("swaps its icon for a spinner while pending", () => {
    render(<IconButton icon={Plus} label="Refresh" pending />);
    const button = screen.getByRole("button", { name: "Refresh" });
    expect(button.getAttribute("aria-busy")).toBe("true");
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(button.querySelectorAll("svg")).toHaveLength(1);
    expect(button.querySelector("[data-spinner]")).toBeTruthy();
  });
});
