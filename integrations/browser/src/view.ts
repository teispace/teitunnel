/** What the popup shows, decided without the DOM (tested). */

import type { HostError, Share } from "./host.ts";

/** A local origin in a form both `localhost` and `127.0.0.1` pages compare equal in. */
function sameService(a: string, b: string): boolean {
  const norm = (origin: string) =>
    origin
      .toLowerCase()
      .replace("://127.0.0.1", "://localhost")
      .replace("://[::1]", "://localhost")
      .replace(/\/$/, "");
  return norm(a) === norm(b);
}

/** The share already serving the page's origin, if any. */
export function shareFor(origin: string | null, shares: readonly Share[]): Share | null {
  if (!origin) return null;
  return shares.find((s) => s.status !== "failed" && sameService(s.origin, origin)) ?? null;
}

/** A share's address without the scheme, or where it is while it has none. */
export function shareLabel(share: Share): string {
  if (share.url) return share.url.replace(/^https?:\/\//, "");
  if (share.status === "failed") return share.error ?? "Couldn't start";
  return "Getting a URL…";
}

/** A dot's colour name for a share. */
export function shareTone(share: Share): "live" | "busy" | "error" | "paused" {
  if (share.paused) return "paused";
  if (share.status === "live") return "live";
  if (share.status === "failed") return "error";
  return "busy";
}

/** What an error means for the person, and what they can do about it. */
export function explain(error: HostError): { text: string; action: "openApp" | "setUp" | null } {
  switch (error.code) {
    case "appNotRunning":
      return { text: "Teitunnel isn't running. Open it, then try again.", action: "openApp" };
    case "hostMissing":
      return { text: error.message, action: "setUp" };
    case "disabled":
      return {
        text: "Teitunnel's connection for other programs is off: turn it on in Settings ▸ Integrations.",
        action: "openApp",
      };
    case "declined":
      return { text: "Not allowed in Teitunnel.", action: null };
    default:
      return { text: error.message, action: null };
  }
}
