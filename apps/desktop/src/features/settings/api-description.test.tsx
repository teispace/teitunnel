import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { ApiDescription } from "./api-description";

let calls: string[];

beforeEach(() => {
  calls = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "inspect_openapi_save") {
      return {
        path: "/Users/me/Downloads/teitunnel-openapi.json",
        summary: { requests: 12, skipped: 3, paths: 4, operations: 6, hosts: ["api.xyz.com"] },
      };
    }
    return null;
  });
});

describe("the API description", () => {
  it("saves an OpenAPI file to Downloads", async () => {
    render(
      <QueryClientProvider client={createQueryClient()}>
        <ApiDescription />
      </QueryClientProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save to Downloads" }));
    await waitFor(() => expect(calls).toContain("inspect_openapi_save"));
  });
});
