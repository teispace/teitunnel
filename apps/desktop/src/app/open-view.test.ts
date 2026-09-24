import { describe, expect, it, vi } from "vitest";
import { openView } from "./open-view";

describe("openView", () => {
  it("navigates to the view a link or the control connection asked for", () => {
    const navigate = vi.fn();
    const go = navigate as unknown as Parameters<typeof openView>[1];
    openView({ view: "route", hostname: "app.example.com" }, go);
    openView({ view: "inspector", share: "qs-1" }, go);
    openView({ view: "share", id: null }, go);
    openView({ view: "doctor" }, go);
    openView({ view: "overview" }, go);
    expect(navigate.mock.calls.map(([options]) => options)).toEqual([
      { to: "/routes", search: { route: "app.example.com" } },
      { to: "/inspector", search: { tap: "qs-1" } },
      { to: "/quick-share" },
      { to: "/doctor" },
      { to: "/" },
    ]);
  });
});
