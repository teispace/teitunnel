import { OPEN_APP_URL, type ShareInfo } from "./protocol.ts";

/**
 * The program and arguments that open Teitunnel from Node without a shell (for tools
 * that have no "open URL" API of their own): `open` on macOS, `xdg-open` on Linux,
 * and the URL protocol handler on Windows.
 */
export function openAppCommand(
  platform: NodeJS.Platform = process.platform,
  url: string = OPEN_APP_URL,
): { command: string; args: string[] } {
  if (platform === "darwin") return { command: "open", args: [url] };
  if (platform === "win32") {
    return { command: "rundll32.exe", args: ["url.dll,FileProtocolHandler", url] };
  }
  return { command: "xdg-open", args: [url] };
}

/** A URL without its scheme or trailing slash, for compact lists. */
export function shortUrl(url: string): string {
  return url.replace(/^[a-z]+:\/\//i, "").replace(/\/$/, "");
}

/** A share's one-line name: its address, else what it shares. */
export function shareLabel(share: ShareInfo): string {
  return share.url ? shortUrl(share.url) : shortUrl(share.origin);
}
