import { currentLanguage, t } from "@/lib/i18n";
import type { AccessRule } from "@/lib/ipc/bindings";

const GITHUB = "github:";

/**
 * "Who can sign in" as typed: email addresses, `@domain`s (everyone at that domain) and
 * `github:org` or `github:org/Team name` (the members of a GitHub organization or team),
 * separated by commas, semicolons or new lines; emails and domains also by spaces, since
 * a GitHub team's name can have them. The engine trims, de-duplicates and validates
 * them, and says which entry is wrong.
 */
export function parseAllowed(text: string): AccessRule & { github: string[] } {
  const rule = { emails: [] as string[], emailDomains: [] as string[], github: [] as string[] };
  for (const item of text.split(/[,;\n]+/)) {
    const trimmed = item.trim();
    if (trimmed.toLowerCase().startsWith(GITHUB)) {
      rule.github.push(trimmed.slice(GITHUB.length).trim());
      continue;
    }
    for (const entry of trimmed.split(/\s+/)) {
      if (entry === "") continue;
      if (entry.indexOf("@") > 0) rule.emails.push(entry);
      else rule.emailDomains.push(entry.replace(/^@/, ""));
    }
  }
  return rule;
}

/** A rule as it's typed (the inverse of {@link parseAllowed}). */
export function formatAllowed(rule: AccessRule | null): string {
  if (!rule) return "";
  return [
    ...rule.emails,
    ...rule.emailDomains.map((d) => `@${d}`),
    ...(rule.github ?? []).map((g) => `${GITHUB}${g}`),
  ].join(", ");
}

/** Paths that skip a login, as typed (commas, spaces or new lines between them). */
export function parsePaths(text: string): string[] {
  return text.split(/[\s,;]+/).filter((entry) => entry !== "");
}

/** Paths as they're typed (the inverse of {@link parsePaths}). */
export function formatPaths(paths: readonly string[] | undefined): string {
  return (paths ?? []).join(", ");
}

/** A rule in a sentence fragment, e.g. `me@xyz.com and anyone at @team.io`. */
export function describeAllowed(rule: AccessRule): string {
  const parts = [
    ...rule.emails,
    ...rule.emailDomains.map((domain) => t("access.anyoneAt", { domain })),
    ...(rule.github ?? []).map((group) => t("access.githubMembers", { group })),
  ];
  return new Intl.ListFormat(currentLanguage(), { type: "conjunction" }).format(parts);
}
