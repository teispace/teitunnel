import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { keysForEntity, queryKeys, refresh } from "./query-keys";

describe("keysForEntity", () => {
  it("maps settings changes to the settings prefix", () => {
    expect(keysForEntity("settings")).toEqual([queryKeys.settings.all()]);
    expect(keysForEntity("quickShares")).toEqual([queryKeys.quickShares.all()]);
  });
});

describe("refresh", () => {
  it("resolves only after the touched queries have refetched", async () => {
    const client = new QueryClient();
    let fetched = 0;
    const observer = new QueryObserver(client, {
      queryKey: queryKeys.routes.overview("a"),
      queryFn: async () => {
        await new Promise((resolve) => setTimeout(resolve, 20));
        return ++fetched;
      },
    });
    const unsubscribe = observer.subscribe(() => {});
    await vi.waitFor(() => expect(fetched).toBe(1));
    await refresh(client, queryKeys.routes.all());
    expect(client.getQueryData(queryKeys.routes.overview("a"))).toBe(2);
    unsubscribe();
  });
});
