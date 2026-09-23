/** Helpers to compare catalogs (tests and `scripts/i18n-missing.ts`). */
type Messages = { [key: string]: string | Messages };

/** Every message as `dotted.key → text`. */
export function flatten(messages: Messages, prefix = ""): Map<string, string> {
  const out = new Map<string, string>();
  for (const [key, value] of Object.entries(messages)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === "string") out.set(path, value);
    else for (const [k, v] of flatten(value, path)) out.set(k, v);
  }
  return out;
}

const PLURAL = /_(zero|one|two|few|many|other)$/;

/** A key without its plural suffix: `a.b_one` → `a.b`. */
export const baseKey = (key: string) => key.replace(PLURAL, "");

/** The `{name}` placeholders in a message, sorted. */
export const placeholders = (text: string) =>
  [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1] ?? "").sort();

/** Problems with a translation against the English source; missing keys aren't problems. */
export function problems(source: Messages, translation: Messages): string[] {
  const en = flatten(source);
  const bases = new Set([...en.keys()].map(baseKey));
  const out: string[] = [];
  for (const [key, text] of flatten(translation)) {
    if (!en.has(key) && !(PLURAL.test(key) && bases.has(baseKey(key)))) {
      out.push(`${key}: not in en.json`);
      continue;
    }
    const reference = en.get(key) ?? en.get(`${baseKey(key)}_other`) ?? "";
    const wanted = placeholders(reference).filter((p) => p !== "count");
    const got = placeholders(text).filter((p) => p !== "count");
    if (wanted.join() !== got.join()) {
      out.push(`${key}: placeholders {${got.join("}, {")}} should be {${wanted.join("}, {")}}`);
    }
  }
  return out;
}

/** Keys English has that the translation doesn't (plurals by base key). */
export function missing(source: Messages, translation: Messages): string[] {
  const have = new Set([...flatten(translation).keys()].map(baseKey));
  return [...new Set([...flatten(source).keys()].map(baseKey))].filter((k) => !have.has(k));
}
