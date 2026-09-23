import { currentLanguage, t } from "@/lib/i18n";
import type { AccessRule } from "@/lib/ipc/bindings";

/**
 * "Who can sign in" as typed: email addresses and `@domain`s (everyone at that domain),
 * separated by commas, spaces or new lines. The engine trims, de-duplicates and
 * validates them, and says which entry is wrong.
 */
export function parseAllowed(text: string): AccessRule {
  const rule: AccessRule = { emails: [], emailDomains: [] };
  for (const entry of text.split(/[\s,;]+/)) {
    if (entry === "") continue;
    if (entry.indexOf("@") > 0) rule.emails.push(entry);
    else rule.emailDomains.push(entry.replace(/^@/, ""));
  }
  return rule;
}

/** A rule as it's typed (the inverse of {@link parseAllowed}). */
export function formatAllowed(rule: AccessRule | null): string {
  if (!rule) return "";
  return [...rule.emails, ...rule.emailDomains.map((d) => `@${d}`)].join(", ");
}

/** A rule in a sentence fragment, e.g. `me@xyz.com and anyone at @team.io`. */
export function describeAllowed(rule: AccessRule): string {
  const parts = [
    ...rule.emails,
    ...rule.emailDomains.map((domain) => t("access.anyoneAt", { domain })),
  ];
  return new Intl.ListFormat(currentLanguage(), { type: "conjunction" }).format(parts);
}
