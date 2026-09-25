import { connect as netConnect, type Socket } from "node:net";
import { type Endpoint, type Environment, resolveEndpoint } from "./endpoint.ts";
import { ControlError, isNotListening } from "./errors.ts";
import { frame, LineReader } from "./framing.ts";
import {
  type ApplyParams,
  type ApplyResult,
  type ClientInfo,
  type ControlEvent,
  type DoctorIssue,
  ErrorCode,
  type EventType,
  type HelloResult,
  type Method,
  type Methods,
  MUTATIONS,
  type PlanInfo,
  PROTOCOL_VERSION,
  type PreviewParams,
  type RoutesList,
  type RoutesParams,
  type ShareInfo,
  type StartShare,
  type Status,
  type View,
} from "./protocol.ts";

export type ConnectionState = "disconnected" | "connecting" | "connected" | "closed";

export interface ControlClientOptions {
  /** Who is connecting: shown to the person when a change needs their approval. */
  client: ClientInfo;
  /** Where to look for the app (default: this process's platform and environment). */
  environment?: Environment;
  /** The app's data folder (default: the platform's, or `TEITUNNEL_DATA_DIR`). */
  dataDir?: string;
  /** Events to subscribe to after every connect: a list, `"all"`, or none (default). */
  events?: EventType[] | "all";
  /** Keep reconnecting when the app isn't running or the connection drops (default: on). */
  reconnect?: boolean;
  /** Reconnect delays: doubling from `initialMs` up to `maxMs`, with jitter. */
  backoff?: { initialMs: number; maxMs: number };
  /** How long a read-only request may take (default 60 s, as the app allows). */
  requestTimeoutMs?: number;
  /** How long a change may take, including the person's answer (default 190 s). */
  mutationTimeoutMs?: number;
  /** How long to wait for the app to accept (default 1.5 s). */
  connectTimeoutMs?: number;
  /** Finds the endpoint (tests). */
  resolve?: () => Promise<Endpoint>;
  /** Diagnostics (never given the token). */
  log?: (message: string) => void;
}

interface Pending {
  resolve: (value: unknown) => void;
  reject: (error: ControlError) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface Response {
  id?: unknown;
  method?: unknown;
  params?: unknown;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

type Listener<T> = (value: T) => void;

/**
 * A connection to the running Teitunnel app over its local control connection
 * (newline-delimited JSON-RPC 2.0 on a Unix socket or named pipe; `hello` with the
 * install's token first). Reconnects with backoff, re-subscribing to events, until
 * `close()`.
 */
export class ControlClient {
  readonly #options: ControlClientOptions;
  #socket: Socket | undefined;
  #state: ConnectionState = "disconnected";
  #hello: HelloResult | undefined;
  #lastError: ControlError | undefined;
  #nextId = 1;
  #pending = new Map<number, Pending>();
  #events = new Set<Listener<ControlEvent>>();
  #states = new Set<Listener<ConnectionState>>();
  #connecting: Promise<HelloResult> | undefined;
  #retry: ReturnType<typeof setTimeout> | undefined;
  #attempt = 0;

  constructor(options: ControlClientOptions) {
    this.#options = options;
  }

  /** Connects, resolving once `hello` is answered. */
  static async connect(options: ControlClientOptions): Promise<ControlClient> {
    const client = new ControlClient({ reconnect: false, ...options });
    await client.connect();
    return client;
  }

  get state(): ConnectionState {
    return this.#state;
  }

  /** The app's answer to `hello` while connected. */
  get hello(): HelloResult | undefined {
    return this.#hello;
  }

  /** Why the last attempt failed, if it did. */
  get lastError(): ControlError | undefined {
    return this.#lastError;
  }

  /** Calls `listener` for every event; returns a function that stops it. */
  onEvent(listener: Listener<ControlEvent>): () => void {
    this.#events.add(listener);
    return () => this.#events.delete(listener);
  }

  /** Calls `listener` when the connection's state changes; returns a function that stops it. */
  onState(listener: Listener<ConnectionState>): () => void {
    this.#states.add(listener);
    return () => this.#states.delete(listener);
  }

  /**
   * Connects now. With `reconnect` on, a failure also schedules another attempt, and
   * later drops reconnect by themselves.
   */
  connect(): Promise<HelloResult> {
    if (this.#state === "closed") return Promise.reject(new ControlError("disconnected"));
    if (this.#state === "connected" && this.#hello) return Promise.resolve(this.#hello);
    if (this.#retry) {
      clearTimeout(this.#retry);
      this.#retry = undefined;
    }
    this.#connecting ??= this.#open().finally(() => {
      this.#connecting = undefined;
    });
    return this.#connecting;
  }

  /** Closes the connection for good (pending requests fail with `disconnected`). */
  close(): void {
    if (this.#retry) clearTimeout(this.#retry);
    this.#retry = undefined;
    this.#setState("closed");
    this.#socket?.destroy();
    this.#socket = undefined;
    this.#failPending(new ControlError("disconnected"));
  }

  /** Sends a request, connecting first if needed. */
  async request<M extends Method>(
    method: M,
    ...params: Methods[M]["params"] extends undefined
      ? [params?: Methods[M]["params"]]
      : [params: Methods[M]["params"]]
  ): Promise<Methods[M]["result"]> {
    if (this.#state !== "connected") await this.connect();
    const timeout = MUTATIONS.has(method)
      ? (this.#options.mutationTimeoutMs ?? 190_000)
      : (this.#options.requestTimeoutMs ?? 60_000);
    return (await this.#send(method, params[0], timeout)) as Methods[M]["result"];
  }

  /** The app, accounts, this machine's tunnels and every share. */
  status(): Promise<Status> {
    return this.request("status");
  }

  /** Every share: the app's, terminals' and domain shares. */
  listShares(): Promise<ShareInfo[]> {
    return this.request("shares.list");
  }

  /** Shares a local service (the person approves it in the app). */
  startShare(share: StartShare): Promise<ShareInfo> {
    return this.request("shares.start", share);
  }

  /** Stops a share by its id, URL or hostname (the person approves it in the app). */
  async stopShare(id: string): Promise<void> {
    await this.request("shares.stop", { id });
  }

  /** This machine's routes in an account. */
  listRoutes(params?: RoutesParams): Promise<RoutesList> {
    return this.request("routes.list", params);
  }

  /** Plans a change for review; nothing changes. */
  previewRoutes(params: PreviewParams): Promise<PlanInfo> {
    return this.request("routes.preview", params);
  }

  /** Applies a reviewed plan by its fingerprint (the person approves it in the app). */
  applyRoutes(params: ApplyParams): Promise<ApplyResult> {
    return this.request("routes.apply", params);
  }

  /** Brings the app's window to a view. */
  async open(view: View): Promise<void> {
    await this.request("open", view);
  }

  /** Runs the Doctor's checks. */
  runDoctor(): Promise<DoctorIssue[]> {
    return this.request("doctor.run");
  }

  #setState(state: ConnectionState): void {
    if (this.#state === state) return;
    if (this.#state === "closed") return;
    this.#state = state;
    for (const listener of this.#states) listener(state);
  }

  #log(message: string): void {
    this.#options.log?.(message);
  }

  async #open(): Promise<HelloResult> {
    this.#setState("connecting");
    try {
      const endpoint = await (this.#options.resolve?.() ??
        resolveEndpoint(this.#options.environment, this.#options.dataDir));
      const socket = await this.#dial(endpoint.path);
      if (this.#state === "closed") {
        socket.destroy();
        throw new ControlError("disconnected");
      }
      this.#attach(socket);
      const hello = (await this.#send(
        "hello",
        { protocol: PROTOCOL_VERSION, token: endpoint.token, client: this.#options.client },
        this.#options.connectTimeoutMs ?? 5000,
      )) as HelloResult;
      this.#hello = hello;
      this.#attempt = 0;
      this.#lastError = undefined;
      this.#setState("connected");
      this.#log(`connected to Teitunnel ${hello.app.version}`);
      const events = this.#options.events;
      if (events) {
        await this.#send(
          "events.subscribe",
          events === "all" ? undefined : { events },
          this.#options.requestTimeoutMs ?? 60_000,
        );
      }
      return hello;
    } catch (error) {
      const failure = error instanceof ControlError ? error : new ControlError("notRunning");
      this.#lastError = failure;
      this.#socket?.destroy();
      this.#socket = undefined;
      this.#hello = undefined;
      this.#setState("disconnected");
      this.#scheduleReconnect(failure);
      throw failure;
    }
  }

  #dial(path: string): Promise<Socket> {
    return new Promise((resolve, reject) => {
      const socket = netConnect({ path });
      const timer = setTimeout(() => {
        socket.destroy();
        reject(new ControlError("notRunning"));
      }, this.#options.connectTimeoutMs ?? 1500);
      socket.once("connect", () => {
        clearTimeout(timer);
        socket.removeAllListeners("error");
        resolve(socket);
      });
      socket.once("error", (error) => {
        clearTimeout(timer);
        this.#log(`couldn't connect: ${(error as { code?: string }).code ?? "error"}`);
        reject(new ControlError(isNotListening(error) ? "notRunning" : "disconnected", undefined));
      });
    });
  }

  #attach(socket: Socket): void {
    this.#socket = socket;
    const reader = new LineReader();
    socket.on("data", (data: Buffer) => {
      let lines: string[];
      try {
        lines = reader.push(data);
      } catch {
        this.#log("a message from Teitunnel was too large; reconnecting");
        socket.destroy(new Error("too large"));
        return;
      }
      for (const line of lines) this.#receive(line);
    });
    socket.on("error", (error) => {
      this.#log(`connection error: ${(error as { code?: string }).code ?? "error"}`);
    });
    socket.on("close", () => {
      if (this.#socket !== socket) return;
      this.#socket = undefined;
      this.#hello = undefined;
      this.#failPending(new ControlError("disconnected"));
      if (this.#state === "closed") return;
      const wasConnected = this.#state === "connected";
      this.#setState("disconnected");
      if (wasConnected) this.#scheduleReconnect(new ControlError("disconnected"));
    });
  }

  #receive(line: string): void {
    let message: Response;
    try {
      message = JSON.parse(line) as Response;
    } catch {
      return;
    }
    if (message.method === "event" && message.id === undefined) {
      const event = message.params as ControlEvent | undefined;
      if (event && typeof event.type === "string") {
        for (const listener of this.#events) listener(event);
      }
      return;
    }
    if (typeof message.id !== "number") {
      // An answer to no request (too many connections, hello timed out…): the app
      // closes the connection next.
      if (message.error) this.#lastError = ControlError.fromRpc(message.error);
      return;
    }
    const pending = this.#pending.get(message.id);
    if (!pending) return;
    this.#pending.delete(message.id);
    clearTimeout(pending.timer);
    if (message.error) pending.reject(ControlError.fromRpc(message.error));
    else pending.resolve(message.result);
  }

  #send(method: string, params: unknown, timeoutMs: number): Promise<unknown> {
    const socket = this.#socket;
    if (!socket) return Promise.reject(this.#lastError ?? new ControlError("notRunning"));
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new ControlError("timeout"));
      }, timeoutMs);
      this.#pending.set(id, { resolve, reject, timer });
      const message: Record<string, unknown> = { jsonrpc: "2.0", id, method };
      if (params !== undefined) message["params"] = params;
      socket.write(frame(message));
    });
  }

  #failPending(error: ControlError): void {
    for (const [id, pending] of this.#pending) {
      clearTimeout(pending.timer);
      pending.reject(error);
      this.#pending.delete(id);
    }
  }

  #scheduleReconnect(reason: ControlError): void {
    if (this.#options.reconnect === false || this.#state === "closed" || this.#retry) return;
    // A version mismatch won't fix itself by retrying.
    if (reason.kind === "unsupportedProtocol" || reason.code === ErrorCode.unsupportedProtocol) {
      return;
    }
    const { initialMs, maxMs } = this.#options.backoff ?? { initialMs: 500, maxMs: 30_000 };
    const base = Math.min(maxMs, initialMs * 2 ** this.#attempt);
    const delay = Math.round(base / 2 + (Math.random() * base) / 2);
    this.#attempt = Math.min(this.#attempt + 1, 30);
    this.#retry = setTimeout(() => {
      this.#retry = undefined;
      this.connect().catch(() => {
        // Scheduled again by #open.
      });
    }, delay);
    // Waiting to reconnect never keeps Node running on its own.
    this.#retry.unref?.();
  }
}
