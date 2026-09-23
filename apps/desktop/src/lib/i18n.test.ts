import { afterEach, describe, expect, it } from "vitest";
import { pickLanguage, setLanguage, t } from "./i18n";

afterEach(() => setLanguage("en"));

describe("i18n", () => {
  it("picks the closest catalog and falls back to English", () => {
    const available = ["de", "en", "pt-BR"];
    expect(pickLanguage(["de-CH", "fr"], available)).toBe("de");
    expect(pickLanguage(["pt-BR"], available)).toBe("pt-BR");
    expect(pickLanguage(["pt-PT", "de"], available)).toBe("de");
    expect(pickLanguage(["ja"], available)).toBe("en");
  });

  it("fills in placeholders and formats numbers", () => {
    expect(t("common.cancel")).toBe("Cancel");
    expect(t("common.cancel", { unused: 1 })).toBe("Cancel");
    expect(t("drift.added", { hostname: "a.xyz.com", service: "localhost:3000" })).toBe(
      "a.xyz.com was added → localhost:3000",
    );
  });

  it("picks the plural form and formats the count", () => {
    expect(t("tunnels.routes", { count: 1 })).toBe("1 route");
    expect(t("tunnels.routes", { count: 0 })).toBe("0 routes");
    expect(t("tunnels.routes", { count: 1234 })).toBe("1,234 routes");
  });
});
