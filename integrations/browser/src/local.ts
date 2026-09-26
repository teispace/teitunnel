/**
 * Whether a page can be shared: only one served from this computer or a private network
 * (the app checks again). Mirrors `teitunnel_core::browser_host::local_origin`.
 */

function privateIpv4(host: string): boolean {
  const parts = host.split(".").map(Number);
  if (parts.length !== 4 || parts.some((p) => !Number.isInteger(p) || p < 0 || p > 255)) {
    return false;
  }
  const [a = 0, b = 0] = parts;
  return (
    a === 127 ||
    a === 10 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 169 && b === 254)
  );
}

function localIpv6(host: string): boolean {
  const bare = host.replace(/^\[|\]$/g, "").toLowerCase();
  return bare === "::1" || /^f[cd][0-9a-f]{0,2}:/.test(bare);
}

/** The origin to share for `page`, e.g. `http://localhost:5173`, or `null`. */
export function localOrigin(page: string | undefined): string | null {
  if (!page) return null;
  let url: URL;
  try {
    url = new URL(page);
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  const host = url.hostname.toLowerCase();
  const local =
    host === "localhost" ||
    host.endsWith(".localhost") ||
    privateIpv4(host) ||
    (host.startsWith("[") && localIpv6(host));
  return local ? `${url.protocol}//${url.host}` : null;
}
