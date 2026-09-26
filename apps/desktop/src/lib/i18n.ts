/**
 * Translations. English (`locales/en.json` at the repository root, shared with
 * the Rust core) is the source: every key exists there, and
 * `MessageKey` is derived from it, so a typo is a type error. Other languages are JSON
 * files with the same keys, contributed by the community; a missing key falls back to
 * English. The language follows the system's (macOS: per-app language in System
 * Settings), like native apps.
 *
 * Messages use `{name}` placeholders. A key with `_one` / `_other` (and, per language,
 * `_zero` `_two` `_few` `_many`) variants is a plural: `t("…", { count })` picks the
 * variant with `Intl.PluralRules`, and `{count}` is formatted for the language.
 */
import en from "@locales/en.json";

type Catalog = typeof en;
type Leaves<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string ? `${P}${K}` : Leaves<T[K], `${P}${K}.`>;
}[keyof T & string];
type AllKeys = Leaves<Catalog>;
type PluralBase<K> = K extends `${infer B}_other` ? B : never;
type PluralVariant = `${string}_${"zero" | "one" | "two" | "few" | "many" | "other"}`;
type PlatformVariant = `${string}@${string}`;
type BaseKeys = Exclude<AllKeys, PlatformVariant>;

/**
 * A message's key, e.g. `routes.empty.title`. Plurals are named without the suffix;
 * platform variants (`key@windows`) are picked automatically.
 */
export type MessageKey = Exclude<BaseKeys, PluralVariant> | PluralBase<BaseKeys>;
export type Vars = Record<string, string | number>;

type Messages = { [key: string]: string | Messages };

const loaders = import.meta.glob<{ default: Messages }>([
  "../../../../locales/*.json",
  "!../../../../locales/en.json",
]);

/** Languages with a catalog, e.g. `["de", "en", "fr"]`. */
export const languages = [
  "en",
  ...Object.keys(loaders).map((path) => path.replace(/^.*\/(.+)\.json$/, "$1")),
].sort();

let locale = "en";
/** The platform whose wording variants apply (`key@windows`), e.g. "PC" for "Mac". */
let platform: string | null = null;

/** Uses `key@<name>` variants where a message has them (call once, at startup). */
export function setPlatformVariant(name: string | null) {
  platform = name;
}
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

/** Loads the catalog for `language` (before the first render); tests pass `catalog`. */
export async function setLanguage(language: string, catalog?: Messages) {
  const loader = loaders[`../../../../locales/${language}.json`];
  active = catalog ?? (loader ? (await loader()).default : en);
  locale = catalog || loader ? language : "en";
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

/** A message in `messages`: the platform's variant if it has one, else the message. */
function pick(messages: Messages, key: string): string | undefined {
  return (platform && lookup(messages, `${key}@${platform}`)) ?? lookup(messages, key);
}

function find(key: string, vars: Vars | undefined): string {
  const count = vars?.["count"];
  if (typeof count === "number") {
    const rule = plurals.select(count);
    const plural =
      pick(active, `${key}_${rule}`) ??
      pick(active, `${key}_other`) ??
      pick(en, `${key}_${new Intl.PluralRules("en").select(count)}`) ??
      pick(en, `${key}_other`);
    if (plural !== undefined) return plural;
  }
  return pick(active, key) ?? pick(en, key) ?? key;
}

/**
 * A message the Rust core produced (`{ key, args }`), in the current language. Its key
 * is checked when the core is compiled, so it's always in the catalog.
 */
export function translate(text: {
  key: string;
  args: Partial<Record<string, string | number | null>>;
}): string {
  const vars: Vars = {};
  for (const [name, value] of Object.entries(text.args)) {
    if (value !== null && value !== undefined) vars[name] = value;
  }
  return t(text.key as MessageKey, vars);
}

/** Text shown as it is, in any language (fixtures, and text that isn't a message). */
export function rawText(text: string): { key: string; args: { text: string } } {
  return { key: "core.raw", args: { text } };
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
