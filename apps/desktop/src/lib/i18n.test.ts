import { afterEach, describe, expect, it } from "vitest";
import { pickLanguage, setLanguage, setPlatformVariant, t, translate } from "./i18n";

afterEach(() => {
  setPlatformVariant(null);
  return setLanguage("en");
});

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

  it("uses a translation's plural rules and numbers, and English for what it lacks", async () => {
    await setLanguage("de", {
      tunnels: { routes_one: "{count} Route", routes_other: "{count} Routen" },
    });
    expect(t("tunnels.routes", { count: 1 })).toBe("1 Route");
    expect(t("tunnels.routes", { count: 1234 })).toBe("1.234 Routen");
    expect(t("common.cancel")).toBe("Cancel");
    expect(document.documentElement.lang).toBe("de");
  });

  it("uses a platform's wording where the message has one", () => {
    expect(t("tunnels.thisMac")).toBe("This Mac");
    setPlatformVariant("windows");
    expect(t("tunnels.thisMac")).toBe("This PC");
    expect(translate({ key: "core.doctor.unusedOwned.title", args: {} })).toBe(
      "This PC's tunnel has no routes",
    );
    // No variant: the message as it is.
    expect(t("common.cancel")).toBe("Cancel");
    setPlatformVariant("linux");
    expect(t("tunnels.thisMac")).toBe("This computer");
  });
});
