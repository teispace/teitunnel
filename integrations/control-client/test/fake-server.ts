/**
 * A stand-in for the app's control server with the same framing and `hello` rules
 * (`crates/control/src/server.rs`), for clients' tests. Exported from the package's
 * `testing` entry so the extensions can use it too.
 */

import { randomBytes } from "node:crypto";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer, type Server, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { frame, LineReader } from "../src/framing.ts";
import { ErrorCode, PROTOCOL_VERSION } from "../src/protocol.ts";

export type Handler = (params: unknown, client: { name: string; version: string }) => unknown;

/** Throw this from a handler to answer with a JSON-RPC error. */
export class RpcFailure extends Error {
  readonly code: number;
  readonly data: unknown;
  constructor(code: number, message: string, data?: unknown) {
    super(message);
    this.code = code;
    this.data = data;
  }
}

interface Connection {
  socket: Socket;
  client?: { name: string; version: string };
  events?: string[];
}

export class FakeApp {
  readonly dataDir: string;
  readonly token: string;
  readonly handlers = new Map<string, Handler>();
  /** Every request received (after `hello`), in order. */
  readonly received: { method: string; params: unknown }[] = [];
  /** `hello` parameters received. */
  readonly hellos: unknown[] = [];
  #server: Server | undefined;
  #connections = new Set<Connection>();
  #path: string;

  private constructor(dataDir: string, token: string) {
    this.dataDir = dataDir;
    this.token = token;
    this.#path =
      process.platform === "win32"
        ? `\\\\.\\pipe\\teitunnel-control-${randomBytes(16).toString("hex")}`
        : join(dataDir, "control", "sock");
  }

  /** A data folder with a token, and (unless `listen` is false) a listening server. */
  static async start(options: { listen?: boolean } = {}): Promise<FakeApp> {
    const dataDir = await mkdtemp(join(tmpdir(), "tt-"));
    await mkdir(join(dataDir, "control"), { recursive: true });
    const token = randomBytes(32).toString("hex");
    await writeFile(join(dataDir, "control", "token"), token);
    const app = new FakeApp(dataDir, token);
    app.handlers.set("status", () => ({
      app: { name: "Teitunnel", version: "9.9.9" },
      accounts: [],
      tunnels: [],
      shares: [],
    }));
    app.handlers.set("events.subscribe", (params) => ({
      events: (params as { events?: string[] } | undefined)?.events ?? [
        "sharesChanged",
        "routesChanged",
        "requestArrived",
      ],
    }));
    if (options.listen !== false) await app.listen();
    return app;
  }

  /** Starts (or restarts) listening. */
  async listen(): Promise<void> {
    if (process.platform === "win32") {
      await writeFile(join(this.dataDir, "control", "pipe"), this.#path);
    }
    const server = createServer((socket) => this.#serve(socket));
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(this.#path, () => resolve());
    });
    this.#server = server;
  }

  /** Stops listening and drops every connection (the app quit). */
  async stop(): Promise<void> {
    for (const connection of this.#connections) connection.socket.destroy();
    this.#connections.clear();
    const server = this.#server;
    this.#server = undefined;
    if (server) await new Promise<void>((resolve) => server.close(() => resolve()));
  }

  /** Stops and removes the data folder. */
  async dispose(): Promise<void> {
    await this.stop();
    await rm(this.dataDir, { recursive: true, force: true });
  }

  /** Sends an event to every connection subscribed to its type. */
  emit(event: { type: string; [key: string]: unknown }): void {
    for (const connection of this.#connections) {
      if (connection.events?.includes(event.type)) {
        connection.socket.write(frame({ jsonrpc: "2.0", method: "event", params: event }));
      }
    }
  }

  /** Clients connected and past `hello`. */
  get clients(): number {
    return [...this.#connections].filter((c) => c.client).length;
  }

  #serve(socket: Socket): void {
    const connection: Connection = { socket };
    this.#connections.add(connection);
    const reader = new LineReader();
    socket.on("close", () => this.#connections.delete(connection));
    socket.on("error", () => {});
    socket.on("data", (data: Buffer) => {
      let lines: string[];
      try {
        lines = reader.push(data);
      } catch {
        socket.end(
          frame({
            jsonrpc: "2.0",
            id: null,
            error: { code: ErrorCode.tooLarge, message: "Message too large." },
          }),
        );
        return;
      }
      for (const line of lines) void this.#handle(connection, line);
    });
  }

  async #handle(connection: Connection, line: string): Promise<void> {
    const { socket } = connection;
    const request = JSON.parse(line) as { id?: unknown; method: string; params?: unknown };
    const answer = (body: object) =>
      socket.write(frame({ jsonrpc: "2.0", id: request.id ?? null, ...body }));
    if (!connection.client) {
      const hello = request.params as {
        protocol: number;
        token: string;
        client: { name: string; version: string };
      };
      this.hellos.push(hello);
      if (request.method !== "hello" || hello?.token !== this.token) {
        answer({ error: { code: ErrorCode.unauthorized, message: "Wrong token." } });
        socket.end();
        return;
      }
      if (hello.protocol !== PROTOCOL_VERSION) {
        answer({
          error: {
            code: ErrorCode.unsupportedProtocol,
            message: "Unsupported protocol.",
            data: { supported: [PROTOCOL_VERSION] },
          },
        });
        socket.end();
        return;
      }
      connection.client = hello.client;
      answer({
        result: {
          protocol: PROTOCOL_VERSION,
          app: { name: "Teitunnel", version: "9.9.9" },
          approved: false,
          methods: [...this.handlers.keys()],
          events: ["sharesChanged", "routesChanged", "requestArrived"],
        },
      });
      return;
    }
    this.received.push({ method: request.method, params: request.params });
    if (request.method === "events.subscribe") {
      connection.events = (request.params as { events?: string[] } | undefined)?.events ?? [
        "sharesChanged",
        "routesChanged",
        "requestArrived",
      ];
    }
    const handler = this.handlers.get(request.method);
    if (!handler) {
      answer({
        error: { code: ErrorCode.methodNotFound, message: `No method called ${request.method}.` },
      });
      return;
    }
    try {
      const result = await handler(request.params, connection.client);
      answer({ result: result ?? {} });
    } catch (error) {
      if (error instanceof RpcFailure) {
        answer({ error: { code: error.code, message: error.message, data: error.data } });
      } else {
        answer({ error: { code: ErrorCode.internal, message: String(error) } });
      }
    }
  }
}
