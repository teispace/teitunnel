import type {
  BodyView,
  ExchangeDetail,
  ExchangeKind,
  ExchangeQuery,
  ExchangeRow,
  HeaderView,
  InspectedRoute,
  InspectorSettings,
  InspectorSettingsPatch,
  KnownTap,
  PlanView,
  TapPatch,
  TapView,
  WebhookSender,
} from "@/lib/ipc/bindings";

/**
 * Dev-only inspector fixtures for the browser preview (see `mock-ipc.ts`) and tests: a
 * Quick Share and an inspected route with a few dozen requests (JSON APIs, a form, an
 * upload, an image, a Stripe webhook, a WebSocket, an event stream, a failure and a
 * replay), masked like the real inspector until revealed.
 */
const MASK = "[redacted]";
const base = Date.UTC(2026, 8, 25, 9, 41, 0);

export const SHARE_TAP = "qs-1";
export const ROUTE_TAP = "rt-docs";

const emptyProtection = {
  password: false,
  secretLink: false,
  basicUser: null,
  bearerTokens: 0,
  ipAllow: [],
  ipDeny: [],
  agentPresets: [],
  agentPatterns: [],
  bypass: [],
};

export function mockTaps(): TapView[] {
  return [
    {
      id: SHARE_TAP,
      scope: { kind: "quickShare", shareId: SHARE_TAP },
      name: "quiet-river-lamp-orbit.trycloudflare.com",
      origin: "http://localhost:5173",
      publicUrl: "https://quiet-river-lamp-orbit.trycloudflare.com",
      address: "http://127.0.0.1:49152",
      startedAt: base - 12 * 60_000,
      capturing: true,
      paused: null,
      protection: { ...emptyProtection, bypass: ["/webhooks/*"] },
      hostHeader: "localhost:5173",
      sseKeepaliveSecs: 25,
      stubs: [
        {
          method: "POST",
          path: "/webhooks/stripe",
          mode: "whenUnreachable",
          status: 200,
          headers: [["Content-Type", "application/json"]],
          body: '{"received":true}',
        },
      ],
      headerRules: { request: [], response: [], cors: false },
      network: {},
      faults: [],
      watchedPaths: ["/webhooks/*"],
      idleStopMinutes: null,
      requests: 42,
      comments: false,
    },
    {
      id: ROUTE_TAP,
      scope: {
        kind: "route",
        accountId: "acc-personal",
        hostname: "docs.teispace.com",
        path: null,
      },
      name: "docs.teispace.com",
      origin: "http://localhost:4321",
      publicUrl: "https://docs.teispace.com",
      address: "http://127.0.0.1:49153",
      startedAt: base - 40 * 60_000,
      capturing: true,
      paused: null,
      protection: emptyProtection,
      hostHeader: null,
      sseKeepaliveSecs: 25,
      stubs: [],
      headerRules: { request: [], response: [], cors: false },
      network: {},
      faults: [],
      watchedPaths: [],
      idleStopMinutes: null,
      requests: 6,
      comments: false,
    },
  ];
}

export function mockKnownTaps(): KnownTap[] {
  return [
    ...mockTaps().map((tap) => ({
      id: tap.id,
      scope: tap.scope,
      name: tap.name,
      origin: tap.origin,
      running: true,
    })),
    {
      id: "qs-0",
      scope: { kind: "quickShare", shareId: "qs-0" },
      name: "misty-harbor-violet-echo.trycloudflare.com",
      origin: "http://localhost:8000",
      running: false,
    },
  ];
}

export function mockInspectedRoutes(): InspectedRoute[] {
  return [
    {
      accountId: "acc-personal",
      hostname: "docs.teispace.com",
      path: null,
      tunnelId: null,
      originalOrigin: "http://localhost:4321",
      access: null,
      lensUrl: "http://127.0.0.1:49153",
      owner: "app",
      createdAt: base - 40 * 60_000,
    },
  ];
}

interface Spec {
  method: string;
  path: string;
  status: number | null;
  ms: number | null;
  bytes: number | null;
  type: string | null;
  kind?: ExchangeKind;
  webhook?: WebhookSender;
  failed?: boolean;
  replayOf?: string;
  local?: boolean;
  tap?: string;
}

const specs: Spec[] = [
  { method: "GET", path: "/", status: 200, ms: 38, bytes: 4_812, type: "text/html" },
  {
    method: "GET",
    path: "/assets/index-3f9a1c.js",
    status: 200,
    ms: 12,
    bytes: 182_400,
    type: "text/javascript",
  },
  {
    method: "GET",
    path: "/assets/index-9b1e.css",
    status: 200,
    ms: 9,
    bytes: 24_310,
    type: "text/css",
  },
  {
    method: "GET",
    path: "/api/session",
    status: 200,
    ms: 64,
    bytes: 312,
    type: "application/json",
  },
  {
    method: "GET",
    path: "/api/projects?limit=20",
    status: 200,
    ms: 121,
    bytes: 8_904,
    type: "application/json",
  },
  {
    method: "POST",
    path: "/api/projects",
    status: 201,
    ms: 187,
    bytes: 402,
    type: "application/json",
  },
  { method: "GET", path: "/logo.png", status: 200, ms: 7, bytes: 68, type: "image/png" },
  { method: "POST", path: "/login", status: 302, ms: 96, bytes: 0, type: null },
  { method: "POST", path: "/upload", status: 200, ms: 842, bytes: 96, type: "application/json" },
  {
    method: "POST",
    path: "/webhooks/stripe",
    status: 200,
    ms: 54,
    bytes: 17,
    type: "application/json",
    webhook: "stripe",
  },
  {
    method: "GET",
    path: "/api/projects/7/metrics",
    status: 500,
    ms: 1_204,
    bytes: 88,
    type: "application/json",
  },
  {
    method: "PATCH",
    path: "/api/projects/7",
    status: 422,
    ms: 71,
    bytes: 140,
    type: "application/json",
  },
  { method: "GET", path: "/ws", status: 101, ms: 61_882, bytes: 0, type: null, kind: "webSocket" },
  {
    method: "GET",
    path: "/api/stream",
    status: 200,
    ms: 14_210,
    bytes: 2_048,
    type: "text/event-stream",
    kind: "sse",
  },
  { method: "DELETE", path: "/api/projects/3", status: 204, ms: 58, bytes: 0, type: null },
  { method: "GET", path: "/api/report.pdf", status: 404, ms: 11, bytes: 22, type: "text/plain" },
  {
    method: "GET",
    path: "/api/slow-query",
    status: null,
    ms: null,
    bytes: null,
    type: null,
    failed: true,
  },
  {
    method: "POST",
    path: "/webhooks/stripe",
    status: 200,
    ms: 49,
    bytes: 17,
    type: "application/json",
    webhook: "stripe",
    replayOf: "ex-10",
  },
  {
    method: "OPTIONS",
    path: "/api/projects",
    status: 204,
    ms: 3,
    bytes: 0,
    type: null,
    local: true,
  },
  {
    method: "GET",
    path: "/guide/getting-started",
    status: 200,
    ms: 22,
    bytes: 12_004,
    type: "text/html",
    tap: ROUTE_TAP,
  },
  {
    method: "GET",
    path: "/search?q=tunnel",
    status: 200,
    ms: 146,
    bytes: 3_201,
    type: "application/json",
    tap: ROUTE_TAP,
  },
  { method: "GET", path: "/favicon.ico", status: 304, ms: 4, bytes: 0, type: null, tap: ROUTE_TAP },
];

/** Every mock request, newest first; the first `specs` repeat to fill a longer list. */
export function mockRows(count = 48): ExchangeRow[] {
  const rows: ExchangeRow[] = [];
  for (let i = 0; i < count; i++) {
    const spec = specs[i % specs.length] as Spec;
    const n = i + 1;
    const tap = spec.tap ?? SHARE_TAP;
    rows.push({
      id: `ex-${n}`,
      tap,
      seq: n,
      startedAt: base + n * 4_700,
      method: spec.method,
      host: tap === ROUTE_TAP ? "docs.teispace.com" : "quiet-river-lamp-orbit.trycloudflare.com",
      path: spec.path,
      status: spec.status,
      durationMs: spec.ms,
      requestBytes: spec.method === "GET" ? 0 : 214,
      responseBytes: spec.bytes,
      kind: spec.kind ?? (spec.type === "text/event-stream" ? "sse" : "http"),
      state: spec.failed ? "failed" : "complete",
      contentType: spec.type,
      webhook: spec.webhook ?? null,
      replayOf: spec.replayOf ?? null,
      answeredLocally: spec.local ?? false,
      error: spec.failed ? "timed out waiting for the response head" : null,
    });
  }
  return rows.reverse();
}

const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAL0lEQVR4nGNgGAWjYBSMglEwCkbBKBgFo2AUDAMYGBj+M/z/z8DAwMDEgAsw4gIAtpsE/0q8tIsAAAAASUVORK5CYII=";

const text = (value: string, type: string | null, kind: BodyView["kind"]): BodyView => ({
  size: new TextEncoder().encode(value).length,
  captured: new TextEncoder().encode(value).length,
  truncated: false,
  complete: true,
  kind,
  contentType: type,
  encoding: null,
  text: value,
  base64: null,
  decodeError: null,
});

const empty = text("", null, "empty");

/** A request in full, masked unless `reveal`. */
export function mockDetail(id: string, reveal = false): ExchangeDetail {
  const row = mockRows().find((r) => r.id === id) ?? (mockRows()[0] as ExchangeRow);
  const secret = (value: string) => (reveal ? value : MASK);
  const [path, query] = row.path.split("?");
  const requestHeaders: HeaderView[] = [
    { name: "host", value: row.host },
    {
      name: "user-agent",
      value: "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_6) AppleWebKit/605.1.15",
    },
    { name: "accept", value: row.contentType ?? "*/*" },
    { name: "cookie", value: secret("session=eyJhbGciOiJIUzI1NiJ9.c2Vzc2lvbg.4fKq") },
    { name: "cf-connecting-ip", value: "203.0.113.24" },
    { name: "cf-ray", value: "8c1f2a3b4d5e6f70-AMS" },
  ];
  let requestBody = empty;
  let responseBody = row.responseBytes ? text("ok", "text/plain", "text") : empty;
  if (row.method === "POST" && row.path === "/api/projects") {
    requestBody = text('{"name":"Launch","visibility":"team"}', "application/json", "json");
    requestHeaders.push({ name: "authorization", value: secret("Bearer sk_live_51H8x2QmZ") });
  }
  if (row.path === "/login") {
    requestBody = text(
      "email=team%40teispace.com&remember=on",
      "application/x-www-form-urlencoded",
      "form",
    );
  }
  if (row.path === "/upload") {
    const boundary = "----WebKitFormBoundary7MA4YWxk";
    requestBody = text(
      `--${boundary}\r\nContent-Disposition: form-data; name="title"\r\n\r\nQuarterly report\r\n--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="report.csv"\r\nContent-Type: text/csv\r\n\r\nmonth,visits\nJuly,1204\nAugust,1377\r\n--${boundary}--\r\n`,
      `multipart/form-data; boundary=${boundary}`,
      "multipart",
    );
  }
  if (row.webhook) {
    requestHeaders.push({
      name: "stripe-signature",
      value: secret(
        "t=1758793260,v1=5257a869e7ecebeda32affa62cdca3fa51cad7e77a0e56ff536d0ce8e108d8bd",
      ),
    });
    requestBody = text(
      '{"id":"evt_1Q2w3E","type":"checkout.session.completed","data":{"object":{"id":"cs_test_a1","amount_total":4900,"currency":"eur"}}}',
      "application/json",
      "json",
    );
  }
  if (row.contentType === "application/json" && row.status !== null) {
    responseBody = text(
      row.status >= 400
        ? '{"error":"Unprocessable","detail":"name must not be empty"}'
        : '{"projects":[{"id":7,"name":"Launch","members":4},{"id":3,"name":"Docs","members":2}],"next":null}',
      "application/json",
      "json",
    );
  }
  if (row.contentType === "image/png") {
    responseBody = {
      ...text("", "image/png", "image"),
      text: null,
      base64: PNG,
      size: 68,
      captured: 68,
    };
  }
  if (row.contentType === "text/html") {
    responseBody = text(
      '<!doctype html>\n<html lang="en">\n  <head><title>Launch</title></head>\n  <body><div id="root"></div></body>\n</html>\n',
      "text/html; charset=utf-8",
      "html",
    );
  }
  const total = (row.durationMs ?? 0) * 1000;
  return {
    view: {
      id: row.id,
      seq: row.seq,
      tap: row.tap,
      kind: row.kind,
      state: row.state,
      startedAtMs: row.startedAt ?? base,
      durationMs: row.durationMs,
      timings:
        row.durationMs === null
          ? { upstreamConnectedUs: 1_200, firstByteUs: null, requestDoneUs: 900, completeUs: null }
          : {
              upstreamConnectedUs: Math.round(total * 0.08),
              firstByteUs: Math.round(total * 0.72),
              requestDoneUs: Math.round(total * 0.05),
              completeUs: total,
            },
      client: {
        ip: "203.0.113.24",
        peer: "127.0.0.1:52144",
        cfRay: "8c1f2a3b4d5e6f70-AMS",
        country: "NL",
      },
      request: {
        method: row.method,
        url: `https://${row.host}${row.path}`,
        path: path ?? "/",
        query: query ?? null,
        host: row.host,
        httpVersion: "HTTP/1.1",
        headers: requestHeaders,
        body: requestBody,
      },
      response:
        row.status === null
          ? null
          : {
              status: row.status,
              statusText:
                {
                  200: "OK",
                  201: "Created",
                  204: "No Content",
                  101: "Switching Protocols",
                  302: "Found",
                  304: "Not Modified",
                  404: "Not Found",
                  422: "Unprocessable Content",
                  500: "Internal Server Error",
                }[row.status] ?? "",
              httpVersion: "HTTP/1.1",
              headers: [
                { name: "content-type", value: row.contentType ?? "text/plain" },
                { name: "content-length", value: String(row.responseBytes ?? 0) },
                { name: "set-cookie", value: secret("sid=9f2c7b1a; Path=/; HttpOnly") },
                { name: "x-powered-by", value: "Express" },
              ],
              body: responseBody,
            },
      responder: row.answeredLocally ? { type: "lens" } : { type: "upstream" },
      error: row.error ? { kind: "timeout", message: row.error } : null,
      stream:
        row.kind === "webSocket"
          ? {
              client: { count: 3, bytes: 96 },
              server: { count: 4, bytes: 312 },
              previews: [],
              frames: [
                ["clientToServer", 120, '{"type":"subscribe","channel":"builds"}'],
                ["serverToClient", 180, '{"type":"subscribed","channel":"builds"}'],
                ["serverToClient", 4_210, '{"type":"build","id":481,"status":"running"}'],
                ["clientToServer", 30_000, null],
                ["serverToClient", 30_002, null],
                ["serverToClient", 58_700, '{"type":"build","id":481,"status":"passed"}'],
              ].map(([direction, ms, preview]) => ({
                atUs: Number(ms) * 1000,
                direction: direction as "clientToServer" | "serverToClient",
                opcode:
                  preview === null ? (direction === "clientToServer" ? "ping" : "pong") : "text",
                fin: true,
                masked: direction === "clientToServer",
                compressed: false,
                size: preview === null ? 0 : String(preview).length,
                preview: preview === null ? "" : String(preview),
                truncated: false,
                closeCode: null,
                closeReason: null,
              })),
              framesDropped: 0,
              closed: false,
            }
          : row.kind === "sse"
            ? {
                client: { count: 0, bytes: 0 },
                server: { count: 3, bytes: 168 },
                previews: [
                  {
                    atUs: 310_000,
                    direction: "serverToClient",
                    kind: "event",
                    size: 52,
                    preview: 'event: progress\ndata: {"step":1,"of":3}',
                    truncated: false,
                    compressed: false,
                    inflated: false,
                  },
                  {
                    atUs: 5_220_000,
                    direction: "serverToClient",
                    kind: "event",
                    size: 52,
                    preview: 'event: progress\ndata: {"step":2,"of":3}',
                    truncated: false,
                    compressed: false,
                    inflated: false,
                  },
                  {
                    atUs: 14_100_000,
                    direction: "serverToClient",
                    kind: "event",
                    size: 48,
                    preview: 'event: done\ndata: {"ok":true}',
                    truncated: false,
                    compressed: false,
                    inflated: false,
                  },
                ],
                frames: [],
                framesDropped: 0,
                closed: true,
              }
            : null,
      replayOf: row.replayOf,
      fault: null,
      redacted: !reveal,
    },
    webhook: row.webhook
      ? {
          provider: row.webhook,
          hasSecret: webhookSecret,
          verification: webhookSecret ? { result: "valid" } : null,
        }
      : null,
    restored: false,
  };
}

let webhookSecret = true;
let settings: InspectorSettings = {
  inspectQuickShares: true,
  keepHistory: true,
  retentionHours: 24,
  idleStopMinutes: null,
  watchedPaths: ["/webhooks/*"],
};
let taps = mockTaps();

const inspectPlan: PlanView = {
  steps: [
    {
      kind: "putConfig",
      description: {
        key: "core.raw",
        args: {
          text: "Point docs.teispace.com at the inspector on this computer (http://127.0.0.1:49153)",
        },
      },
      command: null,
    },
  ],
  warnings: [],
  requiresConfirmation: false,
  fingerprint: "fp-inspect",
};

function list(query: ExchangeQuery) {
  const needle = query.text?.toLowerCase() ?? "";
  const items = mockRows().filter(
    (row) =>
      (!query.tap || row.tap === query.tap) &&
      (!needle ||
        `${row.method} ${row.path} ${row.contentType ?? ""}`.toLowerCase().includes(needle)),
  );
  return { items, next: null };
}

const EXPORT = `curl 'https://quiet-river-lamp-orbit.trycloudflare.com/api/projects' \\
  -X POST \\
  -H 'content-type: application/json' \\
  -H 'authorization: ${MASK}' \\
  --data-raw '{"name":"Launch","visibility":"team"}'`;

/** Answers the inspector's commands, or `undefined` for any other. */
export function inspectorMock(cmd: string, payload: Record<string, unknown>): unknown {
  switch (cmd) {
    case "inspect_settings_get":
      return settings;
    case "inspect_settings_set": {
      const patch = payload["patch"] as InspectorSettingsPatch;
      const pick = <T>(value: T | null | undefined, fallback: T): T => value ?? fallback;
      settings = {
        inspectQuickShares: pick(patch.inspectQuickShares, settings.inspectQuickShares ?? true),
        keepHistory: pick(patch.keepHistory, settings.keepHistory ?? true),
        retentionHours: pick(patch.retentionHours, settings.retentionHours ?? 24),
        idleStopMinutes:
          patch.idleStopMinutes === undefined || patch.idleStopMinutes === null
            ? (settings.idleStopMinutes ?? null)
            : patch.idleStopMinutes || null,
        watchedPaths: pick(patch.watchedPaths, settings.watchedPaths ?? []),
      };
      return settings;
    }
    case "inspect_taps":
      return taps;
    case "inspect_known_taps":
      return mockKnownTaps();
    case "inspect_routes":
      return mockInspectedRoutes();
    case "inspect_exchanges":
      return list(payload["query"] as ExchangeQuery);
    case "inspect_exchange":
      return mockDetail(String(payload["id"]), payload["reveal"] === true);
    case "inspect_webhook_verify":
      return mockDetail(String(payload["id"])).webhook;
    case "inspect_subscribe":
      return 1;
    case "inspect_unsubscribe":
    case "inspect_clear":
      return null;
    case "inspect_replay":
      return [
        { ...(mockRows()[0] as ExchangeRow), id: "ex-replay", replayOf: String(payload["id"]) },
      ];
    case "inspect_export":
      return EXPORT;
    case "inspect_export_save":
      return "~/Downloads/teitunnel-requests.sh";
    case "inspect_configure": {
      const patch = payload["patch"] as TapPatch;
      taps = taps.map((tap) =>
        tap.id === payload["tap"]
          ? {
              ...tap,
              stubs: patch.stubs ?? tap.stubs,
              faults: patch.faults ?? tap.faults,
              watchedPaths: patch.watchedPaths ?? tap.watchedPaths,
              capturing: patch.capturing ?? tap.capturing,
            }
          : tap,
      );
      return taps.find((tap) => tap.id === payload["tap"]);
    }
    case "inspect_protect":
      return {
        protection: taps.find((tap) => tap.id === payload["tap"])?.protection ?? emptyProtection,
        secretLinkKey: null,
        bearerToken: null,
      };
    case "inspect_metrics":
      return {
        requests: 42,
        status: { informational: 1, success: 33, redirect: 3, clientError: 3, serverError: 2 },
        errors: 1,
        blocked: 0,
        stubbed: 0,
        bytesIn: 12_400,
        bytesOut: 480_000,
        activeConnections: 2,
        activeRequests: 0,
        activeStreams: 1,
        latency: { count: 42, p50Ms: 54, p95Ms: 842, p99Ms: 1_204, maxMs: 1_204, meanMs: 131 },
      };
    case "inspect_webhook_secrets":
      return webhookSecret ? ["stripe"] : [];
    case "inspect_webhook_secret_set":
      webhookSecret = true;
      return null;
    case "inspect_webhook_secret_remove":
      webhookSecret = false;
      return null;
    case "inspect_route_preview":
      return {
        change: {
          type: "updateRoute",
          hostname: String(payload["hostname"]),
          path: null,
          route: {
            hostname: String(payload["hostname"]),
            path: null,
            origin: "http://127.0.0.1:49153",
          },
        },
        tunnelId: null,
        plan: inspectPlan,
      };
    case "inspect_route_apply":
      return { type: "applied", tunnelId: null, verify: [], connectorError: null };
    default:
      return undefined;
  }
}
