import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import en from "@locales/en.json";
import { describe, expect, it } from "vitest";
import { flatten, problems } from "./i18n-catalog";

const dir = join(__dirname, "../../../../locales");
const files = readdirSync(dir).filter((f) => f.endsWith(".json") && f !== "en.json");

describe("catalogs", () => {
  it("English plurals always have an _other form", () => {
    for (const key of flatten(en).keys()) {
      if (key.endsWith("_one")) expect(flatten(en).has(key.replace(/_one$/, "_other"))).toBe(true);
    }
  });

  it.each(files.length > 0 ? files : ["(none yet)"])(
    "%s matches English keys and placeholders",
    (file) => {
      if (file === "(none yet)") return;
      const catalog = JSON.parse(readFileSync(join(dir, file), "utf8"));
      expect(problems(en, catalog)).toEqual([]);
    },
  );

  it("flags unknown keys and wrong placeholders", () => {
    expect(
      problems(
        { a: "Hi {name}", n_one: "{count} item", n_other: "{count} items" },
        { a: "Salut {nom}", b: "?", n_few: "{count} položky" },
      ),
    ).toEqual(["a: placeholders {nom} should be {name}", "b: not in en.json"]);
  });
});
