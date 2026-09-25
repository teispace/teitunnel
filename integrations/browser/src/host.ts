/**
 * The link to the Teitunnel app: the `teitunnel` command the browser starts as a native
 * messaging host, which relays requests to the running app. One connection per popup.
 */

/** The host's name (the app's manifest registers it). */
export const HOST = "com.teispace.teitunnel";

/** Why a request failed: `appNotRunning`, `hostMissing`, `declined`, `notLocal`… */
export class HostError extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

/** A share, as the app lists it. */
export interface Share {
  id: string;
  kind: string;
  url: string | null;
  origin: string;
  status: string;
  error?: string;
  paused?: boolean;
}

interface Reply {
  id: number;
  result?: unknown;
  error?: { code: string; message: string };
}

type Pending = { resolve: (value: unknown) => void; reject: (error: HostError) => void };

/** Sends requests to the host and matches the answers. */
export class HostClient {
  readonly #runtime: ExtensionRuntime;
  #port: ExtensionPort | null = null;
  #next = 1;
  readonly #pending = new Map<number, Pending>();

  constructor(runtime: ExtensionRuntime) {
    this.#runtime = runtime;
  }

  #connect(): ExtensionPort {
    if (this.#port) return this.#port;
    const port = this.#runtime.connectNative(HOST);
    port.onMessage.addListener((message) => {
      const reply = message as Reply;
      const waiting = this.#pending.get(reply.id);
      if (!waiting) return;
      this.#pending.delete(reply.id);
      if (reply.error) waiting.reject(new HostError(reply.error.code, reply.error.message));
      else waiting.resolve(reply.result);
    });
    port.onDisconnect.addListener(() => {
      const why = this.#runtime.lastError?.message ?? "";
      const error = /not found|not exist|isn't installed|unknown|forbidden|access/i.test(why)
        ? new HostError(
            "hostMissing",
            "Set up the extension in Teitunnel: Settings ▸ Integrations ▸ Browser Extension (or run `teitunnel browser install`).",
          )
        : new HostError("hostStopped", why || "The connection to Teitunnel closed.");
      for (const waiting of this.#pending.values()) waiting.reject(error);
      this.#pending.clear();
      this.#port = null;
    });
    this.#port = port;
    return port;
  }

  /** Asks the app something through the host. */
  request<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    const id = this.#next++;
    return new Promise<T>((resolve, reject) => {
      this.#pending.set(id, { resolve: (value) => resolve(value as T), reject });
      try {
        this.#connect().postMessage({ id, method, params });
      } catch (error) {
        this.#pending.delete(id);
        reject(new HostError("hostMissing", String(error)));
      }
    });
  }

  close(): void {
    this.#port?.disconnect();
    this.#port = null;
  }
}
