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
    // `key@windows`: a platform's wording of `key`.
    const [message = key] = key.split("@");
    if (!en.has(message) && !(PLURAL.test(message) && bases.has(baseKey(message)))) {
      out.push(`${key}: not in en.json`);
      continue;
    }
    const reference = en.get(key) ?? en.get(message) ?? en.get(`${baseKey(message)}_other`) ?? "";
    const wanted = placeholders(reference).filter((p) => p !== "count");
    const got = placeholders(text).filter((p) => p !== "count");
    if (wanted.join() !== got.join()) {
      out.push(`${key}: placeholders {${got.join("}, {")}} should be {${wanted.join("}, {")}}`);
    }
  }
  return out;
}

/** Words that only fit macOS: a message using one needs `@windows` and `@linux` wording. */
export const MAC_WORDING =
  /\bMac\b|macOS|Finder|⌘|menu bar|Menu bar|keychain|System Settings|Homebrew|\bbrew\b/;

/** English messages that mention macOS without a wording for `platforms`. */
export function missingPlatformWording(
  source: Messages,
  platforms: readonly string[] = ["windows", "linux"],
): string[] {
  const all = flatten(source);
  const out: string[] = [];
  for (const [key, text] of all) {
    // The menu bar is macOS-only (D-063); variants are checked through their base.
    if (key.includes("@") || key.startsWith("core.menu.") || !MAC_WORDING.test(text)) continue;
    for (const platform of platforms) {
      const variant = all.get(`${key}@${platform}`);
      if (variant === undefined) out.push(`${key}@${platform}: missing`);
      else if (MAC_WORDING.test(variant)) out.push(`${key}@${platform}: still says macOS words`);
      else if (placeholders(variant).join() !== placeholders(text).join())
        out.push(`${key}@${platform}: placeholders differ`);
    }
  }
  return out;
}

/** Keys English has that the translation doesn't (plurals by base key). */
export function missing(source: Messages, translation: Messages): string[] {
  const have = new Set([...flatten(translation).keys()].map(baseKey));
  return [...new Set([...flatten(source).keys()].map(baseKey))].filter(
    (k) => !have.has(k) && !k.includes("@"),
  );
}
