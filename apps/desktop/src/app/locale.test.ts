import { describe, expect, it } from "vitest";
import { repairNavigatorLanguage } from "./locale";

function fakeNavigator(language: string, languages: string[]) {
  return { language, languages } as unknown as Navigator;
}

describe("repairNavigatorLanguage", () => {
  it("replaces a POSIX locale that Intl rejects", () => {
    const nav = fakeNavigator("c", ["c", "de-CH"]);
    expect(() => new Intl.NumberFormat(nav.language)).toThrow(RangeError);
    repairNavigatorLanguage(nav);
    expect(() => new Intl.NumberFormat(nav.language)).not.toThrow();
    expect(nav.languages).toEqual(["de-CH"]);
  });

  it("leaves a valid language alone", () => {
    const nav = fakeNavigator("fr-FR", ["fr-FR"]);
    repairNavigatorLanguage(nav);
    expect(nav.language).toBe("fr-FR");
  });
});
