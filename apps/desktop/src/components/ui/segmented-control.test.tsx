import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { SegmentedControl } from "./segmented-control";

function Harness() {
  const [value, setValue] = useState<"a" | "b" | "c">("a");
  return (
    <SegmentedControl
      label="Mode"
      value={value}
      onValueChange={setValue}
      segments={[
        { value: "a", label: "A" },
        { value: "b", label: "B" },
        { value: "c", label: "C" },
      ]}
    />
  );
}

const pressed = () =>
  screen.getAllByRole("radio").find((b) => b.getAttribute("aria-checked") === "true")?.textContent;

describe("SegmentedControl", () => {
  it("selects with arrow keys and space, and never deselects", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    expect(pressed()).toBe("A");
    await user.tab();
    await user.keyboard("{ArrowRight}{ArrowRight} ");
    expect(pressed()).toBe("C");
    fireEvent.click(screen.getByText("C"));
    expect(pressed()).toBe("C");
  });
});
