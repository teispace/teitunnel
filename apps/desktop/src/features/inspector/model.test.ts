import { describe, expect, it } from "vitest";
import { mockRows } from "@/dev/mock-inspector";
import type { ExchangeRow, LiveBatch, Paused } from "@/lib/ipc/bindings";
import { exactPattern } from "./components/breakpoint-rules";
import {
  answerResume,
  diffHeaders,
  diffLines,
  heldChanged,
  heldDraft,
  heldResume,
  hexDump,
  matches,
  newAnswer,
  noFilters,
  parseForm,
  parseMultipart,
  replayInput,
  secondsLeft,
  statusClass,
  statusText,
} from "./model";
import { ExchangeStore, MAX_ROWS } from "./store";

const row = (id: string, extra: Partial<ExchangeRow> = {}): ExchangeRow => ({
  ...(mockRows(1)[0] as ExchangeRow),
  id,
  ...extra,
});

const batch = (exchanges: ExchangeRow[], extra: Partial<LiveBatch> = {}): LiveBatch => ({
  exchanges,
  cleared: [],
  tapsChanged: false,
  lagged: false,
  ...extra,
});

describe("matches", () => {
  const r = row("a", {
    method: "POST",
    path: "/api/x",
    host: "a.test",
    status: 502,
    durationMs: 800,
  });
  it("filters by status class, method, text and duration like the inspector", () => {
    expect(matches(r, noFilters)).toBe(true);
    expect(matches(r, { ...noFilters, status: "5" })).toBe(true);
    expect(matches(r, { ...noFilters, status: "2" })).toBe(false);
    expect(matches(r, { ...noFilters, method: "GET" })).toBe(false);
    expect(matches(r, { ...noFilters, text: "API" })).toBe(true);
    expect(matches(r, { ...noFilters, text: "a.test" })).toBe(true);
    expect(matches(r, { ...noFilters, text: "nope" })).toBe(false);
    expect(matches(r, { ...noFilters, minDurationMs: 1000 })).toBe(false);
    expect(matches(row("p", { status: null }), { ...noFilters, status: "2" })).toBe(false);
    expect(matches(row("f", { state: "failed" }), { ...noFilters, status: "errors" })).toBe(true);
  });
});

describe("ExchangeStore", () => {
  it("puts new requests first, updates known ones in place, and clears by tap", () => {
    const store = new ExchangeStore();
    store.reset([row("b"), row("a")]);
    const added = store.apply(batch([row("c"), row("a", { status: 500 }), row("d")]));
    expect(added).toBe(2);
    expect(store.rows().map((r) => r.id)).toEqual(["d", "c", "b", "a"]);
    expect(store.rows().find((r) => r.id === "a")?.status).toBe(500);
    store.apply(batch([row("e", { tap: "other" })]));
    store.apply(batch([], { cleared: ["other"] }));
    expect(store.rows().map((r) => r.id)).toEqual(["d", "c", "b", "a"]);
    store.apply(batch([], { cleared: [null] }));
    expect(store.rows()).toEqual([]);
  });

  it("keeps only its tap's requests and at most 10,000", () => {
    const store = new ExchangeStore("qs-1");
    store.apply(batch([row("x", { tap: "rt" }), row("y", { tap: "qs-1" })]));
    expect(store.rows().map((r) => r.id)).toEqual(["y"]);
    const many = Array.from({ length: MAX_ROWS + 5 }, (_, i) => row(`r${i}`, { tap: "qs-1" }));
    store.apply(batch(many));
    expect(store.rows()).toHaveLength(MAX_ROWS);
    expect(store.rows()[0]?.id).toBe(`r${MAX_ROWS + 4}`);
  });

  it("keeps no more than a smaller capacity, from a page or live", () => {
    const store = new ExchangeStore(null, 2);
    store.reset([row("c"), row("b"), row("a")]);
    expect(store.rows().map((r) => r.id)).toEqual(["c", "b"]);
    store.apply(batch([row("d")]));
    expect(store.rows().map((r) => r.id)).toEqual(["d", "c"]);
  });

  it("leaves new requests out when a search can't match them", () => {
    const store = new ExchangeStore();
    store.reset([row("a")]);
    expect(store.apply(batch([row("b"), row("a", { status: 404 })]), () => false)).toBe(0);
    expect(store.rows().map((r) => [r.id, r.status])).toEqual([["a", 404]]);
  });
});

describe("bodies", () => {
  it("reads forms and multipart parts", () => {
    expect(parseForm("a=1&b=two%20words&a=3")).toEqual([
      ["a", "1"],
      ["b", "two words"],
      ["a", "3"],
    ]);
    const body =
      '--X\r\nContent-Disposition: form-data; name="title"\r\n\r\nHi\r\n--X\r\nContent-Disposition: form-data; name="file"; filename="a.csv"\r\nContent-Type: text/csv\r\n\r\nx,y\r\n--X--\r\n';
    const parts = parseMultipart(body, "multipart/form-data; boundary=X");
    expect(parts.map((p) => [p.name, p.filename, p.body])).toEqual([
      ["title", null, "Hi"],
      ["file", "a.csv", "x,y"],
    ]);
  });

  it("dumps bytes as hex with an ASCII column", () => {
    expect(hexDump(new TextEncoder().encode("Hello"))).toBe(
      `00000000  48 65 6c 6c 6f${" ".repeat(9)}  ${" ".repeat(23)}  |Hello|`,
    );
  });
});

describe("replayInput", () => {
  const original = {
    method: "POST",
    path: "/hook",
    headers: [
      { name: "authorization", value: "[redacted]" },
      { name: "x-a", value: "1" },
      { name: "host", value: "h" },
    ],
    body: '{"a":1}',
  };
  const draft = {
    method: "POST",
    path: "/hook",
    headers: "authorization: [redacted]\nx-a: 1\nhost: h",
    body: '{"a":1}',
    times: 1,
    resign: false,
  };

  it("sends nothing for an untouched request, so masked values keep their real ones", () => {
    expect(replayInput(original, draft)).toEqual({});
  });

  it("sends only what changed", () => {
    expect(
      replayInput(original, {
        method: "put",
        path: "/hook?x=1",
        headers: "authorization: [redacted]\nx-b: 2\nhost: other",
        body: "{}",
        times: 3,
        resign: true,
      }),
    ).toEqual({
      method: "PUT",
      path: "/hook?x=1",
      setHeaders: [["x-b", "2"]],
      removeHeaders: ["x-a"],
      body: "{}",
      times: 3,
      resign: true,
    });
  });
});

describe("compare", () => {
  it("diffs lines and headers", () => {
    expect(diffLines("a\nb\nc", "a\nc\nd")).toEqual([
      { kind: "same", text: "a" },
      { kind: "removed", text: "b" },
      { kind: "same", text: "c" },
      { kind: "added", text: "d" },
    ]);
    expect(
      diffHeaders(
        [{ name: "A", value: "1" }],
        [
          { name: "a", value: "2" },
          { name: "b", value: "3" },
        ],
      ),
    ).toEqual([
      { name: "a", left: "1", right: "2" },
      { name: "b", left: null, right: "3" },
    ]);
  });
});

describe("breakpoints", () => {
  const held: Paused = {
    exchange: "ex-1",
    tap: "t",
    stage: "request",
    sinceMs: 1_000,
    resumesAtMs: 61_000,
    method: "POST",
    target: "/hooks?x=1",
    host: "app.test",
    status: null,
    headers: [
      ["content-type", "application/json"],
      ["x-a", "1"],
    ],
    body: '{"n":1}',
    bodyLocked: null,
  };

  it("goes on as it is until something changes, then sends only what changed", () => {
    const draft = heldDraft(held);
    expect(draft.headers).toBe("content-type: application/json\nx-a: 1");
    expect(heldChanged(held, draft)).toBe(false);
    expect(heldResume(held, draft)).toEqual({ type: "continue" });
    // Spacing in a header line isn't a change.
    expect(
      heldResume(held, { ...draft, headers: "content-type:application/json\nx-a:  1\n" }),
    ).toEqual({ type: "continue" });

    const edited = {
      ...draft,
      method: "put",
      body: '{"n":2}',
      headers: `${draft.headers}\nx-b: 2`,
    };
    expect(heldChanged(held, edited)).toBe(true);
    expect(heldResume(held, edited)).toEqual({
      type: "edited",
      edit: {
        method: "PUT",
        headers: [
          ["content-type", "application/json"],
          ["x-a", "1"],
          ["x-b", "2"],
        ],
        body: '{"n":2}',
      },
    });
  });

  it("changes an answer's status, never a locked body, and writes answers", () => {
    const answer: Paused = {
      ...held,
      stage: "response",
      status: 200,
      body: null,
      bodyLocked: "binary",
    };
    const draft = heldDraft(answer);
    expect(heldResume(answer, { ...draft, status: "503", body: "x" })).toEqual({
      type: "edited",
      edit: { status: 503 },
    });
    expect(answerResume({ ...newAnswer(), status: "201", body: "{}" })).toEqual({
      type: "answer",
      status: 201,
      headers: [["content-type", "application/json"]],
      body: "{}",
    });
    expect(secondsLeft(held, 30_500)).toBe(31);
    expect(secondsLeft(held, 90_000)).toBe(0);
  });

  it("marks held rows and holds exactly the path asked for", () => {
    const waiting = row("h", { paused: "request", status: null });
    expect(statusText(waiting)).toBe("Held");
    expect(statusClass(waiting)).toBe("text-accent");
    expect(statusClass(row("s", { status: 503, paused: null }))).toBe("text-error");
    expect(exactPattern("/hooks/stripe")).toBe("/hooks/stripe");
    expect(exactPattern("/files/a*b?.txt")).toBe("re:^/files/a\\*b\\?\\.txt$");
  });
});
