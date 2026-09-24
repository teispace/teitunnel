// @vitest-environment node
// The comments API a Snapshot's Worker answers (crates/core/src/engine/snapshot-worker.js),
// against a real SQLite database standing in for D1. Live shares answer the same API
// from Rust (crates/core/src/comments/serve.rs); these check the Worker's copy.
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { FakeD1 } from "./fake-d1";

interface Thread {
  id: string;
  path: string;
  anchor: { selector: string; x: number; y: number } | null;
  resolved: boolean;
  resolvedBy: string | null;
  comments: { author: string; body: string; verified: boolean; byOwner: boolean }[];
}

interface WorkerModule {
  cleanBody(body: unknown): string;
  cleanName(name: unknown): string;
  cleanPath(path: unknown): string;
  cleanAnchor(anchor: unknown): unknown;
  threadsFrom(rows: unknown[]): Thread[];
  sameOriginJson(request: Request): boolean;
  comments(request: Request, env: Record<string, unknown>, now?: number): Promise<Response>;
  default: { fetch(request: Request, env: Record<string, unknown>): Promise<Response> };
}

let worker: WorkerModule;
let db: FakeD1;
let env: Record<string, unknown>;
const SITE = "https://demo.example.com";
const NOW = 1_800_000_000_000;

beforeAll(async () => {
  const url = new URL("../../../../crates/core/src/engine/snapshot-worker.js", import.meta.url);
  worker = (await import(/* @vite-ignore */ url.href)) as WorkerModule;
});

beforeEach(() => {
  db = new FakeD1();
  env = { DB: db, COMMENTS_SITE: "teitunnel-demo", OVERLAY_JS: "/* overlay */" };
});

function post(path: string, body: unknown, headers: Record<string, string> = {}) {
  return new Request(`${SITE}/__teitunnel/comments/${path}`, {
    method: "POST",
    body: typeof body === "string" ? body : JSON.stringify(body),
    headers: {
      "Content-Type": "application/json",
      "Sec-Fetch-Site": "same-origin",
      "CF-Connecting-IP": "203.0.113.9",
      ...headers,
    },
  });
}

async function json(response: Response) {
  return (await response.json()) as Record<string, unknown>;
}

describe("snapshot comments", () => {
  it("checks input like the app does", () => {
    expect(worker.cleanBody("  hi\r\nthere ")).toBe("hi\nthere");
    expect(() => worker.cleanBody(" ")).toThrow();
    expect(() => worker.cleanBody("x".repeat(4001))).toThrow();
    expect(() => worker.cleanBody("a‮b")).toThrow();
    expect(worker.cleanName(" Ana ")).toBe("Ana");
    expect(() => worker.cleanName("a\nb")).toThrow();
    expect(worker.cleanPath("/pricing?x=1")).toBe("/pricing");
    expect(() => worker.cleanPath("//evil.example")).toThrow();
    expect(
      worker.cleanAnchor({ selector: "main", x: 2, y: -1, left: 5, top: 6, vw: 1, vh: 1 }),
    ).toEqual({
      selector: "main",
      x: 1,
      y: 0,
      left: 5,
      top: 6,
      vw: 1,
      vh: 1,
    });
    expect(() =>
      worker.cleanAnchor({ selector: "main", x: Number.NaN, y: 0, left: 0, top: 0 }),
    ).toThrow();
  });

  it("serves the overlay and starts, answers and resolves threads", async () => {
    const script = await worker.comments(
      new Request(`${SITE}/__teitunnel/comments/overlay.js`),
      env,
    );
    expect(script.headers.get("Content-Type")).toContain("text/javascript");
    expect(await script.text()).toBe("/* overlay */");

    const created = await worker.comments(
      post("api/threads", {
        path: "/",
        anchor: { selector: "h1", x: 0.5, y: 0.5, left: 10, top: 20, vw: 1200, vh: 800 },
        body: "<img src=x onerror=alert(1)>",
        author: "Ana",
      }),
      env,
      NOW,
    );
    expect(created.status).toBe(200);
    const thread = (await json(created)) as unknown as Thread;
    expect(thread.comments[0]?.body).toBe("<img src=x onerror=alert(1)>");
    expect(thread.anchor?.selector).toBe("h1");

    const reply = await worker.comments(
      post(`api/threads/${thread.id}/replies`, { body: "Me too", author: "Bo" }),
      env,
      NOW + 1,
    );
    expect(((await json(reply)) as unknown as Thread).comments).toHaveLength(2);

    const resolved = await worker.comments(
      post(`api/threads/${thread.id}/resolve`, { resolved: true, author: "Ana" }),
      env,
      NOW + 2,
    );
    const after = (await json(resolved)) as unknown as Thread;
    expect(after.resolved).toBe(true);
    expect(after.resolvedBy).toBe("Ana");

    const list = await worker.comments(
      new Request(`${SITE}/__teitunnel/comments/api/threads?path=%2F`),
      env,
    );
    const body = await json(list);
    expect((body["threads"] as Thread[]).length).toBe(1);
    expect(body["me"]).toEqual({ verified: false, name: null });
    // What's stored is what the app reads through D1's query endpoint.
    const rows = db.rows("SELECT site, author, email FROM teitunnel_comments ORDER BY created_at");
    expect(rows.map((r) => r["site"])).toEqual(["teitunnel-demo", "teitunnel-demo"]);
    expect(rows.every((r) => r["email"] === null)).toBe(true);
  });

  it("refuses cross-site writes, bad input and unknown threads", async () => {
    const cross = await worker.comments(
      post(
        "api/threads",
        { path: "/", body: "x", author: "A" },
        { "Sec-Fetch-Site": "cross-site" },
      ),
      env,
    );
    expect(cross.status).toBe(403);
    const form = new Request(`${SITE}/__teitunnel/comments/api/threads`, {
      method: "POST",
      body: "path=/&body=x",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
    });
    expect((await worker.comments(form, env)).status).toBe(403);
    const nameless = await worker.comments(post("api/threads", { path: "/", body: "x" }), env);
    expect(nameless.status).toBe(400);
    expect(String((await json(nameless))["error"])).toContain("name");
    const missing = await worker.comments(
      post("api/threads/nope/replies", { body: "x", author: "A" }),
      env,
    );
    expect(missing.status).toBe(404);
    const traversal = await worker.comments(
      post("api/threads/../x/replies", { body: "x", author: "A" }),
      env,
    );
    expect(traversal.status).toBe(404);
  });

  it("rate-limits each visitor", async () => {
    for (let i = 0; i < 10; i++) {
      const ok = await worker.comments(
        post("api/threads", { path: "/", body: `c${i}`, author: "A" }),
        env,
        NOW + i,
      );
      expect(ok.status).toBe(200);
    }
    const limited = await worker.comments(
      post("api/threads", { path: "/", body: "one more", author: "A" }),
      env,
      NOW + 20,
    );
    expect(limited.status).toBe(429);
    const other = await worker.comments(
      post(
        "api/threads",
        { path: "/", body: "hi", author: "B" },
        { "CF-Connecting-IP": "198.51.100.4" },
      ),
      env,
      NOW + 20,
    );
    expect(other.status).toBe(200);
    // A minute later the first visitor may write again.
    const later = await worker.comments(
      post("api/threads", { path: "/", body: "later", author: "A" }),
      env,
      NOW + 61_000,
    );
    expect(later.status).toBe(200);
  });

  it("trusts the Access email only when the Snapshot has Teitunnel's login", async () => {
    const claimed = { "Cf-Access-Authenticated-User-Email": "boss@example.com" };
    const open = await worker.comments(
      post("api/threads", { path: "/", body: "hi", author: "Me" }, claimed),
      env,
      NOW,
    );
    expect(((await json(open)) as unknown as Thread).comments[0]).toMatchObject({
      author: "Me",
      verified: false,
    });
    const guarded = await worker.comments(
      post("api/threads", { path: "/", body: "hi" }, claimed),
      { ...env, ACCESS_IDENTITY: "1" },
      NOW,
    );
    const thread = (await json(guarded)) as unknown as Thread;
    expect(thread.comments[0]).toMatchObject({ author: "boss@example.com", verified: true });
    // The address is kept for the owner but never sent to reviewers.
    expect(JSON.stringify(thread)).not.toContain('"email"');
    expect(db.rows("SELECT email FROM teitunnel_comments WHERE verified = 1")[0]?.["email"]).toBe(
      "boss@example.com",
    );
  });

  it("routes comment paths before the files, behind the password", async () => {
    const assets = {
      fetch: async () =>
        new Response("<h1>site</h1>", { headers: { "Content-Type": "text/html" } }),
    };
    const listed = await worker.default.fetch(
      new Request(`${SITE}/__teitunnel/comments/api/threads`),
      {
        ...env,
        ASSETS: assets,
      },
    );
    expect(listed.headers.get("Content-Type")).toContain("application/json");
    const locked = await worker.default.fetch(
      new Request(`${SITE}/__teitunnel/comments/api/threads`),
      {
        ...env,
        ASSETS: assets,
        PASSWORD_HASH:
          "pbkdf2-sha256$1000$MDEyMzQ1Njc4OWFiY2RlZg$cBg8D2DungRB9k76szThf5ehfyBz991ay6PT8Srwk4M",
      },
    );
    expect(locked.status).toBe(401);
  });
});
