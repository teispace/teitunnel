// Vendored from integrations/control-client/src by scripts/vendor.mjs. Don't edit here.
/**
 * The control connection's wire types, mirroring `crates/control/src/protocol.rs`
 * (protocol version 1). Fields are only ever added: code reading these must ignore
 * fields and event types it doesn't know.
 */

/** The protocol version this client speaks. */
export const PROTOCOL_VERSION = 1;

/** The longest message (one line, without its newline) either side accepts. */
export const MAX_MESSAGE = 1024 * 1024;

/** The link that brings the app to the front (and starts it, if it's installed). */
export const OPEN_APP_URL = "teitunnel://open";

/** JSON-RPC and Teitunnel error codes. */
export const ErrorCode = {
  parseError: -32700,
  invalidRequest: -32600,
  methodNotFound: -32601,
  invalidParams: -32602,
  internal: -32603,
  /** `hello` missing, or its token is wrong. */
  unauthorized: -32001,
  /** Too many requests; slow down. */
  rateLimited: -32002,
  /** The person said no (handle quietly). */
  declined: -32003,
  /** The plan touches DNS records Teitunnel didn't create; pass `confirmed: true`. */
  needsConfirmation: -32004,
  /** Cloudflare changed since the preview; `data` is the new plan. */
  stale: -32005,
  /** The control connection is turned off in Settings. */
  disabled: -32006,
  /** A message was too long. */
  tooLarge: -32007,
  /** What was asked for doesn't exist. */
  notFound: -32008,
  /** The request took too long. */
  timeout: -32009,
  /** The app speaks another protocol version; `data.supported` lists the ones it does. */
  unsupportedProtocol: -32010,
} as const;

/** Who is connecting (shown to the person when approving). Names are self-declared. */
export interface ClientInfo {
  /** A short, stable name, e.g. `vscode`, `raycast`, `jetbrains`. */
  name: string;
  version: string;
}

export interface AppInfo {
  name: string;
  version: string;
}

export interface HelloResult {
  protocol: number;
  app: AppInfo;
  /** The person allowed this client to make changes without asking each time. */
  approved: boolean;
  methods: string[];
  events: string[];
}

export interface AccountInfo {
  id: string;
  name: string;
}

export interface TunnelInfo {
  accountId: string;
  id: string;
  name: string;
  isDefault: boolean;
  state: string;
}

export type ShareKind = "quick" | "terminal" | "domain";

export interface ShareInfo {
  /** Pass this to `shares.stop`. */
  id: string;
  kind: ShareKind;
  url: string | null;
  origin: string;
  /** `starting`, `live`, `reconnecting`, `failed` or `unknown`. */
  status: string;
  error?: string;
  startedAt: number;
  expiresAt: number | null;
  requests: number | null;
  accountId: string | null;
}

export interface Status {
  app: AppInfo;
  accounts: AccountInfo[];
  tunnels: TunnelInfo[];
  shares: ShareInfo[];
}

export type HostHeader = { mode: "auto" } | { mode: "off" } | { mode: "set"; value: string };

export interface StartShare {
  /** A port (`"3000"`), `host:port` or URL. */
  origin: string;
  stopAfterSeconds?: number;
  hostHeader?: HostHeader;
}

export interface RoutesParams {
  /** Account id or name (needed when several are connected). */
  account?: string;
}

export interface RouteInfo {
  hostname: string;
  path: string | null;
  origin: string;
  status: string;
  statusText: string;
  login: string | null;
  connect: string | null;
  tunnelId: string | null;
  tunnelName: string | null;
  temporary: boolean;
}

export interface RoutesList {
  account: AccountInfo;
  tunnels: TunnelInfo[];
  routes: RouteInfo[];
}

export interface PreviewParams {
  account?: string;
  tunnel?: string;
  /** The app's `Change`, e.g. `{type: "addRoute", route: {hostname, origin}}`. */
  change: unknown;
}

export interface StepInfo {
  kind: string;
  description: string;
  command: string | null;
}

export interface PlanInfo {
  accountId: string;
  steps: StepInfo[];
  warnings: unknown[];
  requiresConfirmation: boolean;
  fingerprint: string;
}

export interface ApplyParams extends PreviewParams {
  fingerprint: string;
  confirmed?: boolean;
}

export interface ApplyResult {
  outcome: "applied" | "rolledBack" | "partiallyApplied";
  error: string | null;
  leftovers: string[];
  verify: string[];
  connectorError: string | null;
}

export type View =
  | { view: "overview" }
  | { view: "route"; hostname: string }
  | { view: "share"; id?: string }
  | { view: "inspector"; share: string }
  | { view: "doctor" };

export interface DoctorIssue {
  id: string;
  check: string;
  severity: "error" | "warning" | "info" | string;
  accountId: string | null;
  subject: string;
  title: string;
  detail: string;
}

/** Event types this client knows (the app may send others: ignore them). */
export type EventType = "sharesChanged" | "routesChanged" | "requestArrived";

export type ControlEvent =
  | { type: "sharesChanged"; id?: string | null }
  | { type: "routesChanged"; accountId?: string | null }
  | {
      type: "requestArrived";
      /** The share's id (a route's hostname for inspected routes). */
      share: string;
      method: string;
      /** Path and query, with secrets masked. */
      path: string;
      status: number | null;
      durationMs: number | null;
    };

/** Each method's parameters and result. */
export interface Methods {
  status: { params: undefined; result: Status };
  "shares.list": { params: undefined; result: ShareInfo[] };
  "shares.start": { params: StartShare; result: ShareInfo };
  "shares.stop": { params: { id: string }; result: Record<string, never> };
  "routes.list": { params: RoutesParams | undefined; result: RoutesList };
  "routes.preview": { params: PreviewParams; result: PlanInfo };
  "routes.apply": { params: ApplyParams; result: ApplyResult };
  open: { params: View; result: Record<string, never> };
  "doctor.run": { params: undefined; result: DoctorIssue[] };
  "events.subscribe": {
    params: { events?: EventType[] } | undefined;
    result: { events: string[] };
  };
}

export type Method = keyof Methods;

/** Methods that change something: the person approves them in the app. */
export const MUTATIONS: ReadonlySet<Method> = new Set([
  "shares.start",
  "shares.stop",
  "routes.apply",
]);
