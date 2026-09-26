// The Worker every Teitunnel Snapshot runs.
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
// - DB (D1) with COMMENTS_SITE (plain text, the Worker's name): comments. The Worker
//   serves the overlay (OVERLAY_JS, appended to this module when it's uploaded) and the
//   same JSON API as a live share under /__teitunnel/comments/ (crates/core/src/comments).
//   ACCESS_IDENTITY ("1") trusts Cloudflare Access's email header: only set when the
//   Snapshot has Teitunnel's Access login.
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

// ---------- comments ----------

const COMMENTS_BASE = "/__teitunnel/comments/";
const MAX_BODY = 4000;
const MAX_NAME = 80;
const MAX_PATH = 1024;
const MAX_SELECTOR = 512;
const MAX_PER_THREAD = 200;
const MAX_PER_SITE = 2000;
const MAX_REQUEST = 16 * 1024;
const WRITES_PER_MINUTE = 10;
const WRITES_PER_HOUR = 60;
// Identical to COMMENTS_SCHEMA in crates/core/src/comments/remote.rs (a test checks).
const SCHEMA = [
  "CREATE TABLE IF NOT EXISTS teitunnel_comments (id TEXT PRIMARY KEY, site TEXT NOT NULL, thread TEXT NOT NULL, path TEXT NOT NULL, anchor TEXT, author TEXT NOT NULL, email TEXT, verified INTEGER NOT NULL DEFAULT 0, by_owner INTEGER NOT NULL DEFAULT 0, body TEXT NOT NULL, created_at INTEGER NOT NULL, resolved_at INTEGER, resolved_by TEXT, client TEXT)",
  "CREATE INDEX IF NOT EXISTS teitunnel_comments_site ON teitunnel_comments (site, created_at)",
  "CREATE INDEX IF NOT EXISTS teitunnel_comments_client ON teitunnel_comments (client, created_at)",
];
const COLUMNS =
  "id, thread, path, anchor, author, verified, by_owner, body, created_at, resolved_at, resolved_by";

class Refusal extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

// Control characters (newline and tab allowed where `lines`) and bidirectional overrides.
function hasControl(text, lines) {
  for (const c of text) {
    const n = c.codePointAt(0);
    if (
      (n < 32 && !(lines && (n === 10 || n === 9))) ||
      (n >= 127 && n <= 159) ||
      (n >= 0x202a && n <= 0x202e) ||
      (n >= 0x2066 && n <= 0x2069)
    ) {
      return true;
    }
  }
  return false;
}

export function cleanBody(body) {
  const text = String(body ?? "")
    .replace(/\r\n?/g, "\n")
    .trim();
  if (!text || Array.from(text).length > MAX_BODY || hasControl(text, true)) {
    throw new Refusal(400, `A comment needs text, at most ${MAX_BODY} characters.`);
  }
  return text;
}

export function cleanName(name) {
  const text = String(name ?? "").trim();
  if (!text || Array.from(text).length > MAX_NAME || hasControl(text, false)) {
    throw new Refusal(400, `Add your name, at most ${MAX_NAME} characters.`);
  }
  return text;
}

export function cleanPath(path) {
  const text = String(path ?? "").split(/[?#]/)[0];
  if (
    !text.startsWith("/") ||
    text.startsWith("//") ||
    text.length > MAX_PATH ||
    text.includes("\\") ||
    hasControl(text, false)
  ) {
    throw new Refusal(400, "That page address isn't valid.");
  }
  return text;
}

export function cleanAnchor(anchor) {
  if (anchor == null) return null;
  const selector = String(anchor.selector ?? "").trim();
  const numbers = [anchor.x, anchor.y, anchor.left, anchor.top].map(Number);
  if (
    Array.from(selector).length > MAX_SELECTOR ||
    hasControl(selector, false) ||
    numbers.some((n) => !Number.isFinite(n))
  ) {
    throw new Refusal(400, "That spot on the page isn't valid.");
  }
  const clamp = (n, lo, hi) => Math.min(hi, Math.max(lo, n));
  const size = (n) => clamp(Math.trunc(Number(n) || 0), 0, 100000);
  return {
    selector,
    x: clamp(numbers[0], 0, 1),
    y: clamp(numbers[1], 0, 1),
    left: Math.round(clamp(numbers[2], 0, 1e7)),
    top: Math.round(clamp(numbers[3], 0, 1e7)),
    vw: size(anchor.vw),
    vh: size(anchor.vh),
  };
}

/** Whether a write comes from a page on this site as JSON. */
export function sameOriginJson(request) {
  const type = (request.headers.get("Content-Type") || "").split(";")[0].trim().toLowerCase();
  if (type !== "application/json") return false;
  const site = request.headers.get("Sec-Fetch-Site");
  if (site) return site.toLowerCase() === "same-origin";
  const origin = request.headers.get("Origin");
  return !origin || origin === new URL(request.url).origin;
}

/** Rows (any order) as threads, oldest first; replies without their thread dropped. */
export function threadsFrom(rows) {
  const sorted = [...rows].sort((a, b) => a.created_at - b.created_at || (a.id < b.id ? -1 : 1));
  const threads = new Map();
  const comment = (r) => ({
    id: r.id,
    author: r.author,
    verified: Boolean(r.verified),
    byOwner: Boolean(r.by_owner),
    body: r.body,
    createdAt: r.created_at,
  });
  for (const r of sorted) {
    if (r.id !== r.thread) continue;
    let anchor = null;
    try {
      anchor = r.anchor ? JSON.parse(r.anchor) : null;
    } catch {
      anchor = null;
    }
    threads.set(r.id, {
      id: r.id,
      path: r.path,
      anchor,
      resolved: r.resolved_at != null,
      resolvedBy: r.resolved_by ?? null,
      resolvedAt: r.resolved_at ?? null,
      createdAt: r.created_at,
      comments: [comment(r)],
    });
  }
  for (const r of sorted) {
    if (r.id !== r.thread) threads.get(r.thread)?.comments.push(comment(r));
  }
  return [...threads.values()];
}

function newId() {
  const bytes = crypto.getRandomValues(new Uint8Array(8));
  return `c${[...bytes]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("")
    .slice(1)}`;
}

async function clientKey(request, site) {
  const ip = request.headers.get("CF-Connecting-IP") || "unknown";
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(`${site}\n${ip}`));
  return toBase64Url(digest).slice(0, 22);
}

async function withSchema(db, run) {
  try {
    return await run();
  } catch (error) {
    if (!String(error?.message ?? error).includes("no such table")) throw error;
    await db.batch(SCHEMA.map((sql) => db.prepare(sql)));
    return run();
  }
}

function commentsJson(status, value) {
  return new Response(JSON.stringify(value), {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
      "X-Robots-Tag": "noindex",
    },
  });
}

function identity(request, env) {
  if (env.ACCESS_IDENTITY !== "1") return null;
  const email = (request.headers.get("Cf-Access-Authenticated-User-Email") || "").trim();
  return email.includes("@") && email.length <= 254 ? email : null;
}

function authorOf(request, env, typed) {
  const email = identity(request, env);
  if (email) {
    let name;
    try {
      name = cleanName(typed);
    } catch {
      name = Array.from(email).slice(0, MAX_NAME).join("");
    }
    return { name, email, verified: 1 };
  }
  return { name: cleanName(typed), email: null, verified: 0 };
}

async function loadThread(db, site, id) {
  const { results } = await db
    .prepare(`SELECT ${COLUMNS} FROM teitunnel_comments WHERE site = ?1 AND thread = ?2`)
    .bind(site, id)
    .all();
  const [thread] = threadsFrom(results);
  if (!thread) throw new Refusal(404, "That comment isn't there any more.");
  return thread;
}

async function limited(db, client, now) {
  const row = await db
    .prepare(
      "SELECT sum(CASE WHEN created_at > ?2 THEN 1 ELSE 0 END) AS minute, count(*) AS hour FROM teitunnel_comments WHERE client = ?1 AND created_at > ?3",
    )
    .bind(client, now - 60_000, now - 3_600_000)
    .first();
  return (row?.minute ?? 0) >= WRITES_PER_MINUTE || (row?.hour ?? 0) >= WRITES_PER_HOUR;
}

/** Answers /__teitunnel/comments/… for a Snapshot with comments. */
export async function comments(request, env, now = Date.now()) {
  const url = new URL(request.url);
  const rest = url.pathname.slice(COMMENTS_BASE.length);
  const db = env.DB;
  const site = env.COMMENTS_SITE;
  if (rest === "overlay.js" && (request.method === "GET" || request.method === "HEAD")) {
    const source = env.OVERLAY_JS ?? (typeof OVERLAY_JS === "string" ? OVERLAY_JS : "");
    return new Response(source, {
      headers: {
        "Content-Type": "text/javascript; charset=utf-8",
        "Cache-Control": "no-cache",
        "X-Content-Type-Options": "nosniff",
      },
    });
  }
  try {
    if (request.method === "GET" && rest === "api/threads") {
      const path = url.searchParams.has("path") ? cleanPath(url.searchParams.get("path")) : null;
      const { results } = await withSchema(db, () =>
        db
          .prepare(
            `SELECT ${COLUMNS} FROM teitunnel_comments WHERE site = ?1 AND (?2 IS NULL OR path = ?2) ORDER BY created_at LIMIT ${MAX_PER_SITE}`,
          )
          .bind(site, path)
          .all(),
      );
      const email = identity(request, env);
      return commentsJson(200, {
        threads: threadsFrom(results),
        me: { verified: Boolean(email), name: email },
      });
    }
    if (request.method !== "POST" || !rest.startsWith("api/")) {
      return commentsJson(404, { error: "Not found" });
    }
    if (!sameOriginJson(request)) {
      return commentsJson(403, { error: "Comments can only be posted from this site." });
    }
    const text = await request.text();
    if (text.length > MAX_REQUEST) throw new Refusal(413, "That comment is too long.");
    let input;
    try {
      input = JSON.parse(text);
    } catch {
      throw new Refusal(400, "That request isn't valid.");
    }
    const client = await clientKey(request, site);
    return await withSchema(db, async () => {
      if (await limited(db, client, now)) {
        throw new Refusal(429, "Too many comments at once. Try again in a minute.");
      }
      const total = await db
        .prepare("SELECT count(*) AS n FROM teitunnel_comments WHERE site = ?1")
        .bind(site)
        .first();
      const full = (total?.n ?? 0) >= MAX_PER_SITE;
      if (rest === "api/threads") {
        const path = cleanPath(input.path);
        const anchor = cleanAnchor(input.anchor);
        const body = cleanBody(input.body);
        const author = authorOf(request, env, input.author);
        if (full) throw new Refusal(409, "There are too many comments here.");
        const id = newId();
        await db
          .prepare(
            "INSERT INTO teitunnel_comments (id, site, thread, path, anchor, author, email, verified, by_owner, body, created_at, client) VALUES (?1, ?2, ?1, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10)",
          )
          .bind(
            id,
            site,
            path,
            anchor ? JSON.stringify(anchor) : null,
            author.name,
            author.email,
            author.verified,
            body,
            now,
            client,
          )
          .run();
        return commentsJson(200, await loadThread(db, site, id));
      }
      const match = /^api\/threads\/([A-Za-z0-9]{1,40})\/(replies|resolve)$/.exec(rest);
      if (!match) return commentsJson(404, { error: "Not found" });
      const [, id, action] = match;
      const thread = await loadThread(db, site, id);
      if (action === "replies") {
        const body = cleanBody(input.body);
        const author = authorOf(request, env, input.author);
        if (full || thread.comments.length >= MAX_PER_THREAD) {
          throw new Refusal(409, "There are too many comments here.");
        }
        await db
          .prepare(
            "INSERT INTO teitunnel_comments (id, site, thread, path, author, email, verified, by_owner, body, created_at, client) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10)",
          )
          .bind(
            newId(),
            site,
            id,
            thread.path,
            author.name,
            author.email,
            author.verified,
            body,
            now,
            client,
          )
          .run();
        return commentsJson(200, await loadThread(db, site, id));
      }
      const resolved = input.resolved === true;
      let by = identity(request, env);
      if (!by) {
        try {
          by = cleanName(input.author);
        } catch {
          by = null;
        }
      }
      await db
        .prepare(
          "UPDATE teitunnel_comments SET resolved_at = ?3, resolved_by = ?4 WHERE site = ?1 AND id = ?2 AND thread = id",
        )
        .bind(site, id, resolved ? now : null, resolved ? by : null)
        .run();
      return commentsJson(200, await loadThread(db, site, id));
    });
  } catch (error) {
    if (error instanceof Refusal) return commentsJson(error.status, { error: error.message });
    return commentsJson(503, { error: "Comments are unavailable right now. Try again later." });
  }
}

export default {
  async fetch(request, env) {
    if (env.PASSWORD_HASH) {
      const blocked = await gate(request, env.PASSWORD_HASH, Math.floor(Date.now() / 1000));
      if (blocked) return blocked;
    }
    if (env.DB && env.COMMENTS_SITE && new URL(request.url).pathname.startsWith(COMMENTS_BASE)) {
      return comments(request, env);
    }
    const response = await env.ASSETS.fetch(request);
    return env.OVERLAY_SRC ? withOverlay(response, env.OVERLAY_SRC) : response;
  },
};
