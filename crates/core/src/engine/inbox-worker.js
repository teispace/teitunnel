// Teitunnel's webhook inbox (docs/research/cloudflare-workers-features.md).
//
// A Worker route on hostname/path* in front of a tunnel. A webhook (POST, PUT, PATCH or
// DELETE) goes straight to the tunnel when nothing is waiting; when the tunnel is down
// (530: this computer is off, 502: cloudflared can't reach the app) or earlier webhooks
// are still waiting (so order is kept), it's stored in D1 and the sender gets 202.
// Teitunnel delivers stored webhooks in order when it's back (crates/core/src/inbox.rs)
// and marks them delivered. Other methods (GET verification handshakes) pass through.
//
// Bounded: bodies up to 512 KB are kept (larger ones pass through or get 413 while
// offline), at most INBOX.maxItems waiting (then 503 with Retry-After, so the sender
// retries later), everything older than INBOX.retentionDays deleted on each write.
// With INBOX.verify and the SIGNING_SECRET binding, only correctly signed webhooks are
// stored (GitHub, Stripe, Standard Webhooks); unsigned ones get 401 while offline.
//
// Keep it small; tested in apps/desktop/src/test/front-workers.test.ts.

const MAX_BODY = 512 * 1024;
const MAX_HEADERS = 16 * 1024;
const QUEUED = new Set(["POST", "PUT", "PATCH", "DELETE"]);
const DROPPED =
  /^(cookie|host|connection|keep-alive|transfer-encoding|upgrade|te|trailer|proxy-.*|cf-.*|x-forwarded-.*|x-real-ip|cdn-loop|content-length)$/i;
// Identical to INBOX_SCHEMA in crates/core/src/engine/front.rs (a test checks).
const SCHEMA = [
  "CREATE TABLE IF NOT EXISTS teitunnel_inbox (seq INTEGER PRIMARY KEY AUTOINCREMENT, inbox TEXT NOT NULL, id TEXT NOT NULL UNIQUE, received_at INTEGER NOT NULL, method TEXT NOT NULL, path TEXT NOT NULL, headers TEXT NOT NULL, body TEXT, size INTEGER NOT NULL, delivered_at INTEGER, status INTEGER, attempts INTEGER NOT NULL DEFAULT 0, error TEXT)",
  "CREATE INDEX IF NOT EXISTS teitunnel_inbox_pending ON teitunnel_inbox (inbox, delivered_at, seq)",
];
const encoder = new TextEncoder();

export function settings(env) {
  const raw = JSON.parse(env.INBOX || "{}");
  return {
    id: String(raw.id || "inbox"),
    maxItems: Math.min(1000, Math.max(1, Number(raw.maxItems) || 500)),
    retentionDays: Math.min(30, Math.max(1, Number(raw.retentionDays) || 7)),
    verify: raw.verify || null,
  };
}

function json(status, value, extra = {}) {
  return new Response(JSON.stringify(value), {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "no-store",
      ...extra,
    },
  });
}

function toBase64(bytes) {
  let binary = "";
  const view = new Uint8Array(bytes);
  for (let i = 0; i < view.length; i += 0x8000) {
    binary += String.fromCharCode(...view.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

function fromBase64(text) {
  return Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
}

function hex(bytes) {
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Headers worth keeping for delivery (signatures included), as [name, value] pairs. */
export function keptHeaders(headers) {
  const kept = [];
  let size = 0;
  for (const [name, value] of headers) {
    if (DROPPED.test(name)) continue;
    size += name.length + value.length;
    if (size > MAX_HEADERS) break;
    kept.push([name, value]);
  }
  return kept;
}

function sameText(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

async function hmac(keyBytes, data) {
  const key = await crypto.subtle.importKey(
    "raw",
    keyBytes,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return crypto.subtle.sign("HMAC", key, data);
}

function concat(...parts) {
  const bytes = parts.map((p) => (typeof p === "string" ? encoder.encode(p) : new Uint8Array(p)));
  const out = new Uint8Array(bytes.reduce((n, b) => n + b.length, 0));
  let at = 0;
  for (const b of bytes) {
    out.set(b, at);
    at += b.length;
  }
  return out;
}

/** Whether the webhook is signed with `secret` the way `provider` signs. */
export async function verified(provider, secret, headers, body, now = Date.now()) {
  if (!secret) return false;
  if (provider === "github") {
    const given = headers.get("X-Hub-Signature-256") || "";
    const expected = `sha256=${hex(await hmac(encoder.encode(secret), body))}`;
    return sameText(given, expected);
  }
  if (provider === "stripe") {
    const parts = Object.fromEntries(
      (headers.get("Stripe-Signature") || "").split(",").map((p) => p.split("=", 2)),
    );
    const t = Number(parts.t);
    if (!Number.isFinite(t) || Math.abs(now / 1000 - t) > 300 || !parts.v1) return false;
    const expected = hex(await hmac(encoder.encode(secret), concat(`${parts.t}.`, body)));
    return sameText(parts.v1, expected);
  }
  if (provider === "standard") {
    const id = headers.get("webhook-id") || "";
    const ts = headers.get("webhook-timestamp") || "";
    if (!id || Math.abs(now / 1000 - Number(ts)) > 300) return false;
    let key;
    try {
      key = fromBase64(secret.replace(/^whsec_/, ""));
    } catch {
      return false;
    }
    const expected = toBase64(await hmac(key, concat(`${id}.${ts}.`, body)));
    return (headers.get("webhook-signature") || "")
      .split(" ")
      .some((sig) => sig.startsWith("v1,") && sameText(sig.slice(3), expected));
  }
  return false;
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

async function waiting(db, inbox) {
  const row = await db
    .prepare("SELECT count(*) AS n FROM teitunnel_inbox WHERE inbox = ?1 AND delivered_at IS NULL")
    .bind(inbox)
    .first();
  return row?.n ?? 0;
}

/** Whether the tunnel's answer means this computer (or its app) isn't there. */
export function unavailable(status) {
  return status === 530 || status === 502;
}

export async function handle(request, env, now = Date.now()) {
  if (!QUEUED.has(request.method)) return fetch(request);
  const config = settings(env);
  const db = env.DB;
  const length = Number(request.headers.get("Content-Length") || 0);
  if (length > MAX_BODY) return fetch(request);
  const body = await request.arrayBuffer();
  if (body.byteLength > MAX_BODY) return fetch(new Request(request, { body }));
  const pending = await withSchema(db, () => waiting(db, config.id));
  if (pending === 0) {
    let response = null;
    try {
      response = await fetch(new Request(request, { body }));
    } catch {
      response = null;
    }
    if (response && !unavailable(response.status)) return response;
  }
  if (
    config.verify &&
    !(await verified(config.verify, env.SIGNING_SECRET, request.headers, body, now))
  ) {
    return json(401, { error: "signature" });
  }
  if (pending >= config.maxItems) {
    return json(503, { error: "inbox full" }, { "Retry-After": "300" });
  }
  const url = new URL(request.url);
  const id = crypto.randomUUID();
  await db.batch([
    db
      .prepare("DELETE FROM teitunnel_inbox WHERE inbox = ?1 AND received_at < ?2")
      .bind(config.id, now - config.retentionDays * 86_400_000),
    db
      .prepare(
        "INSERT INTO teitunnel_inbox (inbox, id, received_at, method, path, headers, body, size) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
      )
      .bind(
        config.id,
        id,
        now,
        request.method,
        url.pathname + url.search,
        JSON.stringify(keptHeaders(request.headers)),
        toBase64(body),
        body.byteLength,
      ),
  ]);
  return json(202, { queued: true, id });
}

export default {
  async fetch(request, env) {
    try {
      return await handle(request, env);
    } catch {
      return json(503, { error: "unavailable" }, { "Retry-After": "60" });
    }
  },
};
