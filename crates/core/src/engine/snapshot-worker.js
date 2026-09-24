// The Worker every Teitunnel Snapshot runs (docs/research/cloudflare-snapshots.md).
//
// It serves the Snapshot's files through the ASSETS binding. Two optional bindings add
// behaviour, and with neither set Cloudflare serves the files without running this
// code at all (run_worker_first is false):
//
// - PASSWORD_HASH (secret): "pbkdf2-sha256$<iterations>$<salt>$<hash>", base64url
//   salt and hash. Every request needs a session cookie signed with a key derived from
//   the hash; the login form posts to /__teitunnel/login. Changing the password
//   invalidates every session. Nothing else is stored.
// - OVERLAY_SRC (plain text): a script URL added to every HTML page (the comments
//   overlay). Only same-document HTML responses are rewritten.
//
// Keep this file small and dependency-free: it is reviewed as a whole, and its logic is
// tested in apps/desktop/src/test/snapshot-worker.test.ts.

const COOKIE = "__Host-teitunnel_snapshot";
const LOGIN_PATH = "/__teitunnel/login";
const SESSION_SECONDS = 7 * 24 * 60 * 60;
const encoder = new TextEncoder();

function fromBase64Url(text) {
  const base64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}

function toBase64Url(bytes) {
  let binary = "";
  for (const b of new Uint8Array(bytes)) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** Compares without stopping at the first difference. */
export function sameBytes(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

/** Parses the stored hash; null when it isn't in the expected form. */
export function parseHash(stored) {
  const parts = String(stored || "").split("$");
  if (parts.length !== 4 || parts[0] !== "pbkdf2-sha256") return null;
  const iterations = Number(parts[1]);
  if (!Number.isInteger(iterations) || iterations < 1 || iterations > 1_000_000) return null;
  try {
    return { iterations, salt: fromBase64Url(parts[2]), hash: fromBase64Url(parts[3]) };
  } catch {
    return null;
  }
}

async function derive(password, salt, iterations, length) {
  const key = await crypto.subtle.importKey("raw", encoder.encode(password), "PBKDF2", false, [
    "deriveBits",
  ]);
  const bits = await crypto.subtle.deriveBits(
    { name: "PBKDF2", hash: "SHA-256", salt, iterations },
    key,
    length * 8,
  );
  return new Uint8Array(bits);
}

/** Whether `password` matches the stored hash. */
export async function checkPassword(password, stored) {
  const parsed = parseHash(stored);
  if (!parsed || typeof password !== "string" || password.length > 1024) return false;
  const derived = await derive(password, parsed.salt, parsed.iterations, parsed.hash.length);
  return sameBytes(derived, parsed.hash);
}

async function signingKey(stored) {
  const material = await crypto.subtle.digest("SHA-256", encoder.encode(`session:${stored}`));
  return crypto.subtle.importKey("raw", material, { name: "HMAC", hash: "SHA-256" }, false, [
    "sign",
  ]);
}

/** A session cookie value valid until `expires` (seconds since the epoch). */
export async function sessionValue(stored, expires) {
  const signature = await crypto.subtle.sign(
    "HMAC",
    await signingKey(stored),
    encoder.encode(`v1.${expires}`),
  );
  return `${expires}.${toBase64Url(signature)}`;
}

/** Whether a session cookie value is signed for `stored` and not expired at `now`. */
export async function validSession(value, stored, now) {
  const [expires, signature] = String(value || "").split(".");
  const seconds = Number(expires);
  if (!signature || !Number.isInteger(seconds) || seconds <= now) return false;
  const expected = await sessionValue(stored, seconds);
  return sameBytes(encoder.encode(expected), encoder.encode(`${expires}.${signature}`));
}

function cookieValue(request, name) {
  const header = request.headers.get("Cookie") || "";
  for (const part of header.split(";")) {
    const [key, ...rest] = part.trim().split("=");
    if (key === name) return rest.join("=");
  }
  return null;
}

/** Where to go after logging in: a path on this site only. */
export function safeNext(next) {
  return typeof next === "string" &&
    next.startsWith("/") &&
    !next.startsWith("//") &&
    !next.includes("\\")
    ? next
    : "/";
}

function escapeHtml(text) {
  return text.replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c],
  );
}

const PRIVATE = {
  "Cache-Control": "no-store",
  "X-Robots-Tag": "noindex",
  "Referrer-Policy": "no-referrer",
  "X-Frame-Options": "DENY",
  "Content-Security-Policy": "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'",
};

export function loginPage(next, failed) {
  const body = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Password required</title><style>body{font:15px -apple-system,system-ui,sans-serif;display:grid;place-items:center;min-height:100vh;margin:0;color:#1d1d1f;background:#f5f5f7}@media(prefers-color-scheme:dark){body{color:#f5f5f7;background:#1d1d1f}}form{display:grid;gap:12px;width:min(320px,90vw)}input,button{font:inherit;padding:8px 10px;border-radius:8px;border:1px solid #8886}button{background:#0a84ff;color:#fff;border:0}p{margin:0}</style></head><body><form method="post" action="${LOGIN_PATH}"><p><strong>This page is password protected.</strong></p>${failed ? "<p>That password isn't right.</p>" : ""}<input type="password" name="password" autocomplete="current-password" autofocus required aria-label="Password"><input type="hidden" name="next" value="${escapeHtml(next)}"><button type="submit">Continue</button></form></body></html>`;
  return new Response(body, {
    status: failed ? 403 : 401,
    headers: { "Content-Type": "text/html; charset=utf-8", ...PRIVATE },
  });
}

/** Checks the password gate; returns a response to send instead, or null to go on. */
export async function gate(request, stored, now) {
  const url = new URL(request.url);
  if (request.method === "POST" && url.pathname === LOGIN_PATH) {
    const form = await request.formData().catch(() => null);
    const next = safeNext(form?.get("next"));
    if (!(await checkPassword(form?.get("password"), stored))) return loginPage(next, true);
    const expires = now + SESSION_SECONDS;
    return new Response(null, {
      status: 303,
      headers: {
        Location: next,
        "Set-Cookie": `${COOKIE}=${await sessionValue(stored, expires)}; Path=/; Max-Age=${SESSION_SECONDS}; HttpOnly; Secure; SameSite=Lax`,
        ...PRIVATE,
      },
    });
  }
  if (await validSession(cookieValue(request, COOKIE), stored, now)) return null;
  return loginPage(safeNext(url.pathname + url.search), false);
}

function withOverlay(response, src) {
  const type = response.headers.get("Content-Type") || "";
  if (!type.startsWith("text/html") || typeof HTMLRewriter === "undefined") return response;
  return new HTMLRewriter()
    .on("head", {
      element(head) {
        head.append(`<script src="${escapeHtml(src)}" defer></script>`, { html: true });
      },
    })
    .transform(response);
}

export default {
  async fetch(request, env) {
    if (env.PASSWORD_HASH) {
      const blocked = await gate(request, env.PASSWORD_HASH, Math.floor(Date.now() / 1000));
      if (blocked) return blocked;
    }
    const response = await env.ASSETS.fetch(request);
    return env.OVERLAY_SRC ? withOverlay(response, env.OVERLAY_SRC) : response;
  },
};
