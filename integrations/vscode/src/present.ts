/**
 * What the status bar, the Teitunnel view and notifications say. Pure: no `vscode`
 * import, so it's tested with `node:test`.
 */

import type {
  ConnectionState,
  ControlEvent,
  RouteInfo,
  ShareInfo,
} from "@teitunnel/control-client";
import { shareLabel, shortUrl } from "@teitunnel/control-client";

export interface StatusBarText {
  text: string;
  tooltip: string;
}

const STATUS_WORDS: Record<string, string> = {
  starting: "starting",
  live: "live",
  reconnecting: "reconnecting",
  failed: "failed",
};

/** The status bar item: live share count, or why there's nothing. */
export function statusBar(state: ConnectionState, shares: readonly ShareInfo[]): StatusBarText {
  if (state !== "connected") {
    return {
      text: "$(debug-disconnect) Teitunnel",
      tooltip:
        state === "connecting"
          ? "Connecting to Teitunnel…"
          : "Teitunnel isn't running. Click to open it.",
    };
  }
  const live = shares.filter((share) => share.status === "live");
  const starting = shares.some((share) => share.status === "starting");
  const failed = shares.some((share) => share.status === "failed");
  const icon = failed ? "$(warning)" : starting ? "$(loading~spin)" : "$(broadcast)";
  const text = shares.length === 0 ? `${icon} Teitunnel` : `${icon} ${live.length}`;
  const lines =
    shares.length === 0
      ? ["No shares. Click to share a port."]
      : shares.map(
          (share) =>
            `${shareLabel(share)} → ${shortUrl(share.origin)} (${STATUS_WORDS[share.status] ?? share.status})`,
        );
  return { text, tooltip: `Teitunnel\n${lines.join("\n")}` };
}

/** A share's row in the Teitunnel view. */
export function shareRow(share: ShareInfo): {
  label: string;
  description: string;
  tooltip: string;
  contextValue: string;
} {
  const kind =
    share.kind === "quick"
      ? "Quick Share"
      : share.kind === "domain"
        ? "On your domain"
        : "From a terminal";
  const status = STATUS_WORDS[share.status] ?? share.status;
  const requests =
    share.requests === null
      ? ""
      : ` · ${share.requests} ${share.requests === 1 ? "request" : "requests"}`;
  return {
    label: shareLabel(share),
    description: `${shortUrl(share.origin)} · ${status}`,
    tooltip: [
      share.url ?? "Waiting for an address…",
      `${kind}, sharing ${share.origin}${requests}`,
      share.error,
    ]
      .filter(Boolean)
      .join("\n"),
    // `share.live` items offer Copy and Open; every share can be stopped.
    contextValue: share.url && share.status === "live" ? "share.live" : "share",
  };
}

/** A route's row in the Teitunnel view. */
export function routeRow(route: RouteInfo): {
  label: string;
  description: string;
  tooltip: string;
} {
  const host = route.path ? `${route.hostname}${route.path}` : route.hostname;
  return {
    label: host,
    description: `${shortUrl(route.origin)} · ${route.statusText}`,
    tooltip: [
      `https://${host}`,
      `→ ${route.origin}`,
      route.tunnelName ? `Tunnel: ${route.tunnelName}` : undefined,
      route.login ? `Login: ${route.login}` : undefined,
    ]
      .filter(Boolean)
      .join("\n"),
  };
}

type RequestEvent = Extract<ControlEvent, { type: "requestArrived" }>;

/**
 * Turns `requestArrived` events into at most one notification per `intervalMs`: the
 * first request is shown alone, a burst as a count.
 */
export class RequestNotifier {
  readonly #intervalMs: number;
  readonly #show: (message: string) => void;
  readonly #now: () => number;
  #last = Number.NEGATIVE_INFINITY;
  #waiting: RequestEvent[] = [];
  #timer: ReturnType<typeof setTimeout> | undefined;
  #label = "";

  constructor(show: (message: string) => void, intervalMs = 5000, now: () => number = Date.now) {
    this.#show = show;
    this.#intervalMs = intervalMs;
    this.#now = now;
  }

  push(event: RequestEvent, label: string): void {
    this.#waiting.push(event);
    this.#label = label;
    if (this.#timer) return;
    const wait = Math.max(0, this.#last + this.#intervalMs - this.#now());
    if (wait === 0) this.flush();
    else this.#timer = setTimeout(() => this.flush(), wait);
  }

  /** Shows what's waiting now. */
  flush(): void {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = undefined;
    const events = this.#waiting.splice(0);
    if (events.length === 0) return;
    this.#last = this.#now();
    const [first] = events;
    if (events.length === 1 && first) {
      const status = first.status === null ? "" : ` → ${first.status}`;
      const took = first.durationMs === null ? "" : ` in ${first.durationMs} ms`;
      this.#show(`${first.method} ${first.path}${status}${took} on ${this.#label}`);
    } else {
      this.#show(`${events.length} requests on ${this.#label}`);
    }
  }

  dispose(): void {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = undefined;
    this.#waiting = [];
  }
}
