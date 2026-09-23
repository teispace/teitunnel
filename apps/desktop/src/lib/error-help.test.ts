import english from "@locales/en.json";
import { describe, expect, it } from "vitest";
import { errorHelp } from "./error-help";
import { flatten } from "./i18n-catalog";

describe("errorHelp", () => {
  it("decides how to help with every error the core can report", () => {
    const keys = [...flatten(english).keys()]
      .filter((key) => key.startsWith("core.error.") && !key.includes("@"))
      .sort();
    expect(Object.keys(errorHelp).sort()).toEqual(keys);
  });
});
