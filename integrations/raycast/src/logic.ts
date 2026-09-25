/** What the commands say and accept. Pure: no `@raycast/api` import, so it's tested with `node:test`. */

import type { DoctorIssue, ShareInfo } from "./control-client/index.ts";
import { shortUrl } from "./control-client/index.ts";

/** A port as typed in the argument: `3000`, `:3000`, `localhost:3000` or a local URL. */
export function parseOrigin(input: string): string | undefined {
  const text = input.trim();
  if (/^\d{1,5}$/.test(text)) return Number(text) > 0 && Number(text) < 65536 ? text : undefined;
  if (/^:\d{1,5}$/.test(text)) return text.slice(1);
  if (/^(https?:\/\/)?[\w.-]+:\d{1,5}\/?$/.test(text)) return text;
  return undefined;
}

const KINDS: Record<string, string> = {
  quick: "Quick Share",
  domain: "On your domain",
  terminal: "From a terminal",
};

/** The row for a share in List Shares. */
export function shareRow(share: ShareInfo): {
  title: string;
  subtitle: string;
  status: string;
  keywords: string[];
} {
  return {
    title: share.url ? shortUrl(share.url) : "Waiting for an address…",
    subtitle: `${shortUrl(share.origin)} · ${KINDS[share.kind] ?? share.kind}`,
    status:
      share.status === "live" && share.requests !== null
        ? `${share.requests} requests`
        : share.status,
    keywords: [share.origin, share.id, ...(share.url ? [share.url] : [])],
  };
}

/** Shares, live first and newest first. */
export function sortShares(shares: readonly ShareInfo[]): ShareInfo[] {
  return [...shares].sort(
    (a, b) =>
      Number(b.status === "live") - Number(a.status === "live") || b.startedAt - a.startedAt,
  );
}

/** Problems, errors first. */
export function sortIssues(issues: readonly DoctorIssue[]): DoctorIssue[] {
  const rank = (s: string) => (s === "error" ? 0 : s === "warning" ? 1 : 2);
  return [...issues].sort((a, b) => rank(a.severity) - rank(b.severity));
}
