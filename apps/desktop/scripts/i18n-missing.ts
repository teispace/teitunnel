// Lists the messages a translation doesn't have yet, and any problems with it.
// Usage: node scripts/i18n-missing.ts <language>   (e.g. de, pt-BR)
import { existsSync, readFileSync } from "node:fs";
import { missing, problems } from "../src/locales/catalog.ts";

const language = process.argv[2];
if (!language) {
  process.stderr.write("Usage: node scripts/i18n-missing.ts <language>\n");
  process.exit(2);
}
const read = (name: string) => JSON.parse(readFileSync(`src/locales/${name}.json`, "utf8"));
const en = read("en");
const translation = existsSync(`src/locales/${language}.json`) ? read(language) : {};
for (const key of missing(en, translation)) process.stdout.write(`missing  ${key}\n`);
const found = problems(en, translation);
for (const problem of found) process.stdout.write(`problem  ${problem}\n`);
process.exit(found.length > 0 ? 1 : 0);
