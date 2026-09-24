// @vitest-environment node
// The Workers Teitunnel puts in front of a route (crates/core/src/engine/): the offline
// page and the webhook inbox, with the tunnel (their fetch) stubbed.
import { createHmac } from "node:crypto";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { FakeD1 } from "./fake-d1";

interface Offline {
  settings(env: Record<string, unknown>): { title: string; message: string; whenAppDown: boolean };
  isOffline(status: number, page: { whenAppDown: boolean }): boolean;
  default: { fetch(request: Request, env: Record<string, unknown>): Promise<Response> };
}

interface Inbox {
  keptHeaders(headers: Headers): [string, string][];
  verified(
    provider: string,
    secret: string,
    headers: Headers,
    body: ArrayBuffer,
    now?: number,
  ): Promise<boolean>;
  handle(request: Request, env: Record<string, unknown>, now?: number): Promise<Response>;
  default: { fetch(request: Request, env: Record<string, unknown>): Promise<Response> };
}

let offline: Offline;
let inbox: Inbox;

beforeAll(async () => {
  const base = "../../../../crates/core/src/engine/";
  offline = (await import(
    /* @vite-ignore */ new URL(`${base}offline-worker.js`, import.meta.url).href
  )) as Offline;
  inbox = (await import(
    /* @vite-ignore */ new URL(`${base}inbox-worker.js`, import.meta.url).href
  )) as Inbox;
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function tunnel(status: number, body = "origin") {
  const calls: Request[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (request: Request) => {
      calls.push(request);
      return new Response(body, { status });
    }),
  );
  return calls;
}

const PAGE = JSON.stringify({
  title: "Back <soon>",
  message: "Line one\nLine two",
  whenAppDown: false,
});
const page = (
  url = "https://app.example.com/",
  headers: Record<string, string> = { Accept: "text/html" },
) => new Request(url, { headers });

describe("offline page", () => {
  it("passes working responses through untouched", async () => {
    tunnel(200, "hello");
    const response = await offline.default.fetch(page(), { PAGE });
    expect(response.status).toBe(200);
    expect(await response.text()).toBe("hello");
    tunnel(502);
    expect((await offline.default.fetch(page(), { PAGE })).status).toBe(502);
  });

  it("shows the person's page when the tunnel is down (530, error 1033)", async () => {
    tunnel(530, "error code: 1033");
    const response = await offline.default.fetch(page(), { PAGE });
    expect(response.status).toBe(503);
    expect(response.headers.get("Retry-After")).toBe("60");
    expect(response.headers.get("Cache-Control")).toBe("no-store");
    const html = await response.text();
    expect(html).toContain("Back &lt;soon&gt;");
    expect(html).toContain("<p>Line one</p><p>Line two</p>");
    expect(html).not.toContain("1033");
  });

  it("answers API calls with JSON and covers the app being down only when asked", async () => {
    tunnel(530);
    const api = await offline.default.fetch(
      page("https://app.example.com/api", { Accept: "application/json" }),
      {
        PAGE,
      },
    );
    expect(api.headers.get("Content-Type")).toContain("application/json");
    expect(offline.isOffline(502, { whenAppDown: false })).toBe(false);
    expect(offline.isOffline(502, { whenAppDown: true })).toBe(true);
    expect(offline.settings({ PAGE: "not json" }).title).toBe("Back soon");
  });
});

function webhook(body: string, headers: Record<string, string> = {}, path = "/hooks/github") {
  return new Request(`https://app.example.com${path}?x=1`, {
    method: "POST",
    body,
    headers: { "Content-Type": "application/json", Cookie: "s=1", "CF-Ray": "abc", ...headers },
  });
}

const env = (db: FakeD1, extra: Record<string, unknown> = {}) => ({
  DB: db,
  INBOX: JSON.stringify({
    id: "tt-inbox-1",
    path: "/hooks/",
    maxItems: 2,
    retentionDays: 7,
    verify: null,
  }),
  ...extra,
});

describe("webhook inbox", () => {
  it("delivers straight to the tunnel when nothing is waiting", async () => {
    const db = new FakeD1();
    const calls = tunnel(200, "ok");
    const response = await inbox.handle(webhook('{"a":1}'), env(db));
    expect(response.status).toBe(200);
    expect(await calls[0]?.text()).toBe('{"a":1}');
    expect(db.rows("SELECT count(*) AS n FROM teitunnel_inbox")[0]?.n).toBe(0);
  });

  it("keeps webhooks while the computer is off, in order, then refuses when full", async () => {
    const db = new FakeD1();
    tunnel(530);
    const first = await inbox.handle(webhook('{"n":1}'), env(db), 1000);
    expect(first.status).toBe(202);
    // The tunnel is back, but one is waiting: the next one queues behind it.
    const calls = tunnel(200);
    const second = await inbox.handle(webhook('{"n":2}'), env(db), 2000);
    expect(second.status).toBe(202);
    expect(calls).toHaveLength(0);
    const full = await inbox.handle(webhook('{"n":3}'), env(db), 3000);
    expect(full.status).toBe(503);
    expect(full.headers.get("Retry-After")).toBe("300");
    const rows = db.rows(
      "SELECT method, path, headers, body, size FROM teitunnel_inbox ORDER BY seq",
    );
    expect(rows.map((r) => Buffer.from(String(r.body), "base64").toString())).toEqual([
      '{"n":1}',
      '{"n":2}',
    ]);
    expect(rows[0]?.path).toBe("/hooks/github?x=1");
    const headers = JSON.parse(String(rows[0]?.headers)) as [string, string][];
    const names = headers.map(([name]) => name);
    expect(names).toContain("content-type");
    expect(names).not.toContain("cookie");
    expect(names).not.toContain("cf-ray");
  });

  it("drops webhooks older than the retention on the next write", async () => {
    const db = new FakeD1();
    tunnel(530);
    await inbox.handle(webhook("{}"), env(db), 0);
    db.db.exec("UPDATE teitunnel_inbox SET delivered_at = 1");
    await inbox.handle(webhook("{}"), env(db), 8 * 86_400_000);
    expect(db.rows("SELECT count(*) AS n FROM teitunnel_inbox")[0]?.n).toBe(1);
  });

  it("passes other methods and large bodies through", async () => {
    const db = new FakeD1();
    const calls = tunnel(530);
    const get = await inbox.handle(new Request("https://app.example.com/hooks/x"), env(db));
    expect(get.status).toBe(530);
    await inbox.handle(webhook("x".repeat(600 * 1024)), env(db));
    expect(calls).toHaveLength(2);
    expect(
      db.rows("SELECT count(*) AS n FROM sqlite_master WHERE name = 'teitunnel_inbox'")[0]?.n,
    ).toBe(0);
  });

  it("keeps only correctly signed webhooks when verifying", async () => {
    const db = new FakeD1();
    tunnel(530);
    const secret = "gh-secret";
    const body = '{"action":"opened"}';
    const signature = `sha256=${createHmac("sha256", secret).update(body).digest("hex")}`;
    const verifying = env(db, {
      INBOX: JSON.stringify({
        id: "tt-inbox-1",
        path: "/hooks/",
        maxItems: 10,
        retentionDays: 7,
        verify: "github",
      }),
      SIGNING_SECRET: secret,
    });
    expect(
      (await inbox.handle(webhook(body, { "X-Hub-Signature-256": "sha256=00" }), verifying)).status,
    ).toBe(401);
    expect(
      (await inbox.handle(webhook(body, { "X-Hub-Signature-256": signature }), verifying)).status,
    ).toBe(202);
  });

  it("checks Stripe and Standard Webhooks signatures", async () => {
    const encoder = new TextEncoder();
    const body = encoder.encode('{"id":"evt_1"}').buffer as ArrayBuffer;
    const now = 1_800_000_000_000;
    const t = String(now / 1000);
    const stripe = createHmac("sha256", "whsec_stripe").update(`${t}.{"id":"evt_1"}`).digest("hex");
    const headers = new Headers({ "Stripe-Signature": `t=${t},v1=${stripe}` });
    expect(await inbox.verified("stripe", "whsec_stripe", headers, body, now)).toBe(true);
    expect(await inbox.verified("stripe", "whsec_stripe", headers, body, now + 600_000)).toBe(
      false,
    );

    const key = Buffer.from("standard-key").toString("base64");
    const signed = createHmac("sha256", Buffer.from(key, "base64"))
      .update(`msg_1.${t}.{"id":"evt_1"}`)
      .digest("base64");
    const standard = new Headers({
      "webhook-id": "msg_1",
      "webhook-timestamp": t,
      "webhook-signature": `v1,${signed}`,
    });
    expect(await inbox.verified("standard", `whsec_${key}`, standard, body, now)).toBe(true);
    expect(await inbox.verified("standard", `whsec_${key}`, standard, body, now + 600_000)).toBe(
      false,
    );
    expect(await inbox.verified("github", "", new Headers(), body)).toBe(false);
  });

  it("answers 503 instead of throwing when storage fails", async () => {
    tunnel(530);
    const broken = {
      prepare: () => ({
        bind: () => ({
          first: async () => {
            throw new Error("boom");
          },
        }),
      }),
    };
    const response = await inbox.default.fetch(webhook("{}"), { DB: broken, INBOX: "{}" });
    expect(response.status).toBe(503);
  });
});
