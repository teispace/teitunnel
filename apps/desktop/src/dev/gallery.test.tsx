import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import Gallery from "./gallery";

describe("gallery", () => {
  it("renders every section without crashing", () => {
    const { getByText } = render(
      <TooltipProvider>
        <Gallery />
      </TooltipProvider>,
    );
    expect(getByText("Buttons")).toBeTruthy();
    expect(getByText("Disclosure")).toBeTruthy();
  });
});
