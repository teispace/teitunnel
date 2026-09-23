/**
 * Translations. English (`locales/en.json`) is the source: every key exists there, and
 * `MessageKey` is derived from it, so a typo is a type error. Other languages are JSON
 * files with the same keys, contributed by the community; a missing key falls back to
 * English. The language follows the system's (macOS: per-app language in System
 * Settings), like native apps.
 *
 * Messages use `{name}` placeholders. A key with `_one` / `_other` (and, per language,
 * `_zero` `_two` `_few` `_many`) variants is a plural: `t("…", { count })` picks the
 * variant with `Intl.PluralRules`, and `{count}` is formatted for the language.
 */
import en from "@/locales/en.json";

type Catalog = typeof en;
type Leaves<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string ? `${P}${K}` : Leaves<T[K], `${P}${K}.`>;
}[keyof T & string];
type AllKeys = Leaves<Catalog>;
type PluralBase<K> = K extends `${infer B}_other` ? B : never;
type PluralVariant = `${string}_${"zero" | "one" | "two" | "few" | "many" | "other"}`;

/** A message's key, e.g. `routes.empty.title`. Plurals are named without the suffix. */
export type MessageKey = Exclude<AllKeys, PluralVariant> | PluralBase<AllKeys>;
export type Vars = Record<string, string | number>;

type Messages = { [key: string]: string | Messages };

const loaders = import.meta.glob<{ default: Messages }>([
  "../locales/*.json",
  "!../locales/en.json",
]);

/** Languages with a catalog, e.g. `["de", "en", "fr"]`. */
export const languages = [
  "en",
  ...Object.keys(loaders).map((path) => path.replace(/^.*\/(.+)\.json$/, "$1")),
].sort();

let locale = "en";
let active: Messages = en;
let plurals = new Intl.PluralRules("en");
let numbers = new Intl.NumberFormat("en");

/** The best catalog for `wanted` (BCP 47 tags, most preferred first), or `en`. */
export function pickLanguage(wanted: readonly string[], available = languages): string {
  for (const tag of wanted) {
    let candidate = tag;
    while (candidate) {
      const match = available.find((a) => a.toLowerCase() === candidate.toLowerCase());
      if (match) return match;
      candidate = candidate.includes("-") ? candidate.slice(0, candidate.lastIndexOf("-")) : "";
    }
  }
  return "en";
}

/** Loads the catalog for `language` (before the first render). */
export async function setLanguage(language: string) {
  const loader = loaders[`../locales/${language}.json`];
  active = loader ? (await loader()).default : en;
  locale = loader ? language : "en";
  plurals = new Intl.PluralRules(locale);
  numbers = new Intl.NumberFormat(locale);
  document.documentElement.lang = locale;
}

/** The language messages are shown in. */
export function currentLanguage() {
  return locale;
}

function lookup(messages: Messages, key: string): string | undefined {
  let node: string | Messages | undefined = messages;
  for (const part of key.split(".")) {
    if (typeof node !== "object") return undefined;
    node = node[part];
  }
  return typeof node === "string" ? node : undefined;
}

function find(key: string, vars: Vars | undefined): string {
  const count = vars?.["count"];
  if (typeof count === "number") {
    const rule = plurals.select(count);
    const plural =
      lookup(active, `${key}_${rule}`) ??
      lookup(active, `${key}_other`) ??
      lookup(en, `${key}_${new Intl.PluralRules("en").select(count)}`) ??
      lookup(en, `${key}_other`);
    if (plural !== undefined) return plural;
  }
  return lookup(active, key) ?? lookup(en, key) ?? key;
}

/** The message for `key` in the current language, with `{name}`s filled in. */
export function t(key: MessageKey, vars?: Vars): string {
  const message = find(key, vars);
  if (!vars) return message;
  return message.replace(/\{(\w+)\}/g, (whole, name: string) => {
    const value = vars[name];
    if (value === undefined) return whole;
    return typeof value === "number" ? numbers.format(value) : value;
  });
}
