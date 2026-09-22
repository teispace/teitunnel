import { describe, expect, it } from "vitest";
import { keysForEntity, queryKeys } from "./query-keys";

describe("keysForEntity", () => {
  it("maps settings changes to the settings prefix", () => {
    expect(keysForEntity("settings")).toEqual([queryKeys.settings.all()]);
  });
});
