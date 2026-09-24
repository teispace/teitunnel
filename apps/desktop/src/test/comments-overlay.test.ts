// The comments overlay reviewers see on a share or Snapshot
// (crates/core/src/comments/overlay.js): its selector and anchor helpers, and the
// overlay itself mounted in jsdom against a fake API.
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import source from "../../../../crates/core/src/comments/overlay.js?raw";

interface Anchor {
  selector: string;
  x: number;
  y: number;
  left: number;
  top: number;
  vw: number;
  vh: number;
}

interface Overlay {
  cssPath(el: Element): string;
  anchorFor(el: Element, x: number, y: number, win?: Window): Anchor;
  resolveAnchor(
    anchor: Anchor | null,
    doc?: Document,
    win?: Window,
  ): { x: number; y: number; exact: boolean } | null;
  relativeTime(ms: number, now?: number): string;
  cleanName(name: unknown): string;
  hashTarget(hash: string): string | null;
  mount(options: Record<string, unknown>): {
    ready: Promise<void>;
    shadow: ShadowRoot;
    host: HTMLElement;
    destroy(): void;
  };
}

const overlay = {} as Overlay;

beforeAll(() => {
  (globalThis as Record<string, unknown>).__TEITUNNEL_COMMENTS_TEST__ = overlay;
  new Function(source)();
  delete (globalThis as Record<string, unknown>).__TEITUNNEL_COMMENTS_TEST__;
});

function rect(el: Element, box: { left: number; top: number; width: number; height: number }) {
  el.getBoundingClientRect = () =>
    ({
      ...box,
      x: box.left,
      y: box.top,
      right: box.left + box.width,
      bottom: box.top + box.height,
      toJSON() {},
    }) as DOMRect;
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("overlay helpers", () => {
  it("builds selectors that find the same element again", () => {
    document.body.innerHTML = `<main><section><p>a</p><p>b</p></section><div id="card"><span>x</span></div><div id="1bad"></div></main>`;
    const second = document.querySelectorAll("p")[1] as Element;
    const path = overlay.cssPath(second);
    expect(document.querySelector(path)).toBe(second);
    expect(path).toContain("p:nth-of-type(2)");
    const span = document.querySelector("#card span") as Element;
    expect(overlay.cssPath(span)).toBe("#card > span:nth-of-type(1)");
    const bad = document.querySelector("[id='1bad']") as Element;
    expect(document.querySelector(overlay.cssPath(bad))).toBe(bad);
  });

  it("anchors a click inside its element and finds the spot again", () => {
    document.body.innerHTML = `<h1>Title</h1>`;
    const h1 = document.querySelector("h1") as Element;
    rect(h1, { left: 100, top: 50, width: 200, height: 40 });
    const anchor = overlay.anchorFor(h1, 150, 60);
    expect(anchor.x).toBeCloseTo(0.25);
    expect(anchor.y).toBeCloseTo(0.25);
    expect(anchor.selector).toContain("h1");
    rect(h1, { left: 0, top: 0, width: 400, height: 80 });
    expect(overlay.resolveAnchor(anchor)).toMatchObject({ x: 100, y: 20, exact: true });
    // The element is gone: the page position is used instead.
    expect(overlay.resolveAnchor({ ...anchor, selector: "#gone" })).toMatchObject({
      x: 150,
      exact: false,
    });
    expect(overlay.resolveAnchor({ ...anchor, selector: "<<bad" })).toMatchObject({ exact: false });
    expect(overlay.resolveAnchor(null)).toBeNull();
  });

  it("formats times and names", () => {
    const now = 10_000_000;
    expect(overlay.relativeTime(now - 10_000, now)).toBe("just now");
    expect(overlay.relativeTime(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(overlay.relativeTime(now - 3 * 3_600_000, now)).toBe("3 hr ago");
    expect(overlay.cleanName("  Ana\u0000 ")).toBe("Ana");
    expect(overlay.cleanName("x".repeat(100))).toHaveLength(80);
    expect(overlay.hashTarget("#__teitunnel-comment=c123")).toBe("c123");
    expect(overlay.hashTarget("#other")).toBeNull();
  });
});

const THREAD = {
  id: "c1",
  path: "/",
  anchor: { selector: "h1", x: 0.5, y: 0.5, left: 10, top: 10, vw: 1000, vh: 800 },
  resolved: false,
  resolvedBy: null,
  resolvedAt: null,
  createdAt: Date.now() - 60_000,
  comments: [
    {
      id: "c1",
      author: "Ana",
      verified: false,
      byOwner: false,
      body: "<img src=x onerror=alert(1)>",
      createdAt: Date.now() - 60_000,
    },
  ],
};

function api() {
  const calls: { method: string; url: string; body: unknown; headers: Record<string, string> }[] =
    [];
  const fetch = vi.fn(async (url: string, init: RequestInit) => {
    const body = init.body ? JSON.parse(String(init.body)) : undefined;
    calls.push({
      method: init.method ?? "GET",
      url,
      body,
      headers: init.headers as Record<string, string>,
    });
    if (url.includes("api/threads?"))
      return new Response(JSON.stringify({ threads: [THREAD], me: { verified: false } }));
    return new Response(
      JSON.stringify({
        ...THREAD,
        id: "c2",
        comments: [{ ...THREAD.comments[0], id: "c2", body: body.body, author: body.author }],
      }),
    );
  });
  return { fetch, calls };
}

describe("overlay", () => {
  it("mounts in a Shadow DOM, shows pins and renders text as text", async () => {
    document.body.innerHTML = `<h1>Hello</h1>`;
    rect(document.querySelector("h1") as Element, { left: 0, top: 0, width: 100, height: 20 });
    const { fetch } = api();
    const mounted = overlay.mount({ fetch, base: "/__teitunnel/comments/", openShadow: true });
    await mounted.ready;
    expect(document.querySelector("[data-teitunnel-comments]")).toBe(mounted.host);
    const fab = mounted.shadow.querySelector("button.fab") as HTMLButtonElement;
    expect(fab.getAttribute("aria-label")).toBe("Comments, 1 open");
    const pin = mounted.shadow.querySelector("button.pin") as HTMLButtonElement;
    expect(pin.getAttribute("aria-label")).toBe("Comment 1 by Ana");
    expect(pin.style.left).toBe("50px");
    pin.click();
    const dialog = mounted.shadow.querySelector(
      "[role=dialog][aria-label='Comment thread']",
    ) as HTMLElement;
    expect(dialog.querySelector(".body")?.textContent).toBe("<img src=x onerror=alert(1)>");
    expect(mounted.shadow.querySelector("img")).toBeNull();
    // Escape closes it.
    dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(mounted.shadow.querySelector("[aria-label='Comment thread']")).toBeNull();
    mounted.destroy();
    expect(document.querySelector("[data-teitunnel-comments]")).toBeNull();
  });

  it("pins a new comment where the reviewer clicked and posts it as JSON", async () => {
    document.body.innerHTML = `<main><h1>Hello</h1></main>`;
    const h1 = document.querySelector("h1") as HTMLElement;
    rect(h1, { left: 0, top: 0, width: 100, height: 20 });
    const { fetch, calls } = api();
    const mounted = overlay.mount({ fetch, base: "/__teitunnel/comments/", openShadow: true });
    await mounted.ready;
    (mounted.shadow.querySelector("button.fab") as HTMLButtonElement).click();
    (mounted.shadow.querySelector(".panel button.primary") as HTMLButtonElement).click();
    expect(mounted.shadow.querySelector(".hint")?.hasAttribute("hidden")).toBe(false);
    h1.dispatchEvent(
      new MouseEvent("click", { bubbles: true, cancelable: true, clientX: 25, clientY: 10 }),
    );
    const form = mounted.shadow.querySelector("[aria-label='New comment'] form") as HTMLFormElement;
    (form.querySelector("input[name=author]") as HTMLInputElement).value = "Bo";
    (form.querySelector("textarea") as HTMLTextAreaElement).value = "Make this bigger";
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(calls.some((c) => c.method === "POST")).toBe(true));
    const posted = calls.find((c) => c.method === "POST");
    expect(posted?.url).toBe("/__teitunnel/comments/api/threads");
    expect(posted?.headers["Content-Type"]).toBe("application/json");
    expect(posted?.body).toMatchObject({ path: "/", body: "Make this bigger", author: "Bo" });
    expect((posted?.body as { anchor: Anchor } | undefined)?.anchor.x).toBeCloseTo(0.25);
    expect(window.localStorage.getItem("teitunnel.comments.name")).toBe("Bo");
    mounted.destroy();
  });
});
