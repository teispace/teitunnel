import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { ListPane, ListRow } from "./list-pane";

const items = ["alpha", "beta", "gamma"];

function Harness() {
  const [selected, setSelected] = useState<string | null>(null);
  return (
    <ListPane
      label="Items"
      items={items}
      getId={(item) => item}
      selectedId={selected}
      onSelect={setSelected}
      renderRow={(item) => <ListRow title={item} />}
    />
  );
}

const selectedText = () =>
  screen.queryAllByRole("option").find((o) => o.getAttribute("aria-selected") === "true")
    ?.textContent;

describe("ListPane", () => {
  it("moves the selection with the keyboard", () => {
    render(<Harness />);
    const list = screen.getByRole("listbox", { name: "Items" });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(selectedText()).toBe("alpha");
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(selectedText()).toBe("beta");
    fireEvent.keyDown(list, { key: "End" });
    expect(selectedText()).toBe("gamma");
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(selectedText()).toBe("gamma");
    fireEvent.keyDown(list, { key: "Home" });
    expect(selectedText()).toBe("alpha");
    expect(list.getAttribute("aria-activedescendant")).toContain("alpha");
  });

  it("selects on mouse down", () => {
    render(<Harness />);
    fireEvent.mouseDown(screen.getByText("gamma"));
    expect(selectedText()).toBe("gamma");
  });
});
