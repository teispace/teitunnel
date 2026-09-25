// @vitest-environment node
// The Worker every Snapshot runs (crates/core/src/engine/snapshot-worker.js): its
// password gate, sessions and redirects, tested with the same Web APIs Workers have.
import { beforeAll, describe, expect, it } from "vitest";

interface WorkerModule {
  parseHash(stored: string): { iterations: number; salt: Uint8Array; hash: Uint8Array } | null;
  checkPassword(password: unknown, stored: string): Promise<boolean>;
  sessionValue(stored: string, expires: number): Promise<string>;
  validSession(value: string | null, stored: string, now: number): Promise<boolean>;
  safeNext(next: unknown): string;
  gate(request: Request, stored: string, now: number): Promise<Response | null>;
  default: {
    fetch(request: Request, env: Record<string, unknown>): Promise<Response>;
  };
}

// The vector `encodes_what_the_worker_expects` checks in Rust (snapshot/password.rs):
// both sides must agree on the format.
const STORED =
  "pbkdf2-sha256$1000$MDEyMzQ1Njc4OWFiY2RlZg$cBg8D2DungRB9k76szThf5ehfyBz991ay6PT8Srwk4M";
const PASSWORD = "correct horse";
const NOW = 1_800_000_000;

let worker: WorkerModule;

beforeAll(async () => {
  const url = new URL("../../../../crates/core/src/engine/snapshot-worker.js", import.meta.url);
  worker = (await import(/* @vite-ignore */ url.href)) as WorkerModule;
});

function login(password: string, next = "/docs/") {
  return new Request("https://preview.example.com/__teitunnel/login", {
    method: "POST",
    body: new URLSearchParams({ password, next }),
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
  });
}

describe("snapshot worker", () => {
  it("checks the password the app hashed", async () => {
    expect(worker.parseHash(STORED)?.iterations).toBe(1000);
    expect(worker.parseHash("md5$1$a$b")).toBeNull();
    expect(worker.parseHash("pbkdf2-sha256$0$a$b")).toBeNull();
    expect(await worker.checkPassword(PASSWORD, STORED)).toBe(true);
    expect(await worker.checkPassword("wrong horse", STORED)).toBe(false);
    expect(await worker.checkPassword(null, STORED)).toBe(false);
    expect(await worker.checkPassword(PASSWORD, "garbage")).toBe(false);
  });

  it("signs sessions that expire and belong to one password", async () => {
    const value = await worker.sessionValue(STORED, NOW + 60);
    expect(await worker.validSession(value, STORED, NOW)).toBe(true);
    expect(await worker.validSession(value, STORED, NOW + 61)).toBe(false);
    expect(await worker.validSession(value, `${STORED}x`, NOW)).toBe(false);
    const [expires, signature] = value.split(".");
    expect(await worker.validSession(`${Number(expires) + 999}.${signature}`, STORED, NOW)).toBe(
      false,
    );
    expect(await worker.validSession(null, STORED, NOW)).toBe(false);
    expect(await worker.validSession("nonsense", STORED, NOW)).toBe(false);
  });

  it("only redirects within the site after logging in", () => {
    expect(worker.safeNext("/docs/?a=1")).toBe("/docs/?a=1");
    expect(worker.safeNext("//evil.example")).toBe("/");
    expect(worker.safeNext("https://evil.example")).toBe("/");
    expect(worker.safeNext("/\\evil.example")).toBe("/");
    expect(worker.safeNext(undefined)).toBe("/");
  });

  it("asks for the password, then lets the visitor in with a cookie", async () => {
    const page = await worker.gate(new Request("https://preview.example.com/docs/"), STORED, NOW);
    expect(page?.status).toBe(401);
    expect(page?.headers.get("Cache-Control")).toBe("no-store");
    expect(await page?.text()).toContain('value="/docs/"');

    const wrong = await worker.gate(login("nope"), STORED, NOW);
    expect(wrong?.status).toBe(403);

    const ok = await worker.gate(login(PASSWORD), STORED, NOW);
    expect(ok?.status).toBe(303);
    expect(ok?.headers.get("Location")).toBe("/docs/");
    const cookie = ok?.headers.get("Set-Cookie") ?? "";
    expect(cookie).toMatch(
      /^__Host-teitunnel_snapshot=\d+\.[\w-]+; Path=\/; Max-Age=\d+; HttpOnly; Secure; SameSite=Lax$/,
    );

    const session = cookie.split(";")[0] ?? "";
    const next = new Request("https://preview.example.com/docs/", {
      headers: { Cookie: `theme=dark; ${session}` },
    });
    expect(await worker.gate(next, STORED, NOW)).toBeNull();
  });

  it("escapes what it echoes into the login page", async () => {
    const page = await worker.gate(
      new Request('https://preview.example.com/"><script>alert(1)</script>'),
      STORED,
      NOW,
    );
    const html = (await page?.text()) ?? "";
    expect(html).not.toContain("<script>alert(1)</script>");
  });

  it("serves the files, behind the password only when one is set", async () => {
    const assets = { fetch: async () => new Response("<h1>site</h1>", { status: 200 }) };
    const open = await worker.default.fetch(new Request("https://preview.example.com/"), {
      ASSETS: assets,
    });
    expect(await open.text()).toBe("<h1>site</h1>");
    const locked = await worker.default.fetch(new Request("https://preview.example.com/"), {
      ASSETS: assets,
      PASSWORD_HASH: STORED,
    });
    expect(locked.status).toBe(401);
  });
});
