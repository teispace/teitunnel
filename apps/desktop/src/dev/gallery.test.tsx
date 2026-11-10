import { QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import Gallery from "./gallery";

describe("gallery", () => {
  it("renders every section without crashing", () => {
    const { getByText } = render(
      <QueryClientProvider client={createQueryClient()}>
        <TooltipProvider>
          <Gallery />
        </TooltipProvider>
      </QueryClientProvider>,
    );
    expect(getByText("Buttons")).toBeTruthy();
    expect(getByText("Disclosure")).toBeTruthy();
  });
});
