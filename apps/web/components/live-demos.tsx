"use client";

import { Check, LoaderCircle, RotateCcw, ShieldCheck } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { card, usePlayOnView } from "./demos";
import { Logo } from "./logo";

interface Request {
  method: string;
  path: string;
  status: number;
  ms: number;
}

// Requests like the inspector's mock traffic (apps/desktop/src/dev/mock-inspector.ts).
const traffic: Request[] = [
  { method: "GET", path: "/", status: 200, ms: 38 },
  { method: "GET", path: "/assets/index-3f9a1c.js", status: 200, ms: 12 },
  { method: "GET", path: "/api/session", status: 200, ms: 64 },
  { method: "POST", path: "/webhooks/stripe", status: 200, ms: 54 },
  { method: "GET", path: "/api/projects?limit=20", status: 200, ms: 121 },
  { method: "POST", path: "/api/projects", status: 201, ms: 187 },
  { method: "GET", path: "/favicon.ico", status: 304, ms: 4 },
  { method: "PATCH", path: "/api/projects/7", status: 422, ms: 71 },
  { method: "GET", path: "/search?q=tunnel", status: 200, ms: 146 },
  { method: "POST", path: "/login", status: 302, ms: 96 },
  { method: "GET", path: "/api/projects/7/metrics", status: 500, ms: 1204 },
  { method: "OPTIONS", path: "/api/projects", status: 204, ms: 3 },
];

const ROWS = 6;

function statusColor(status: number): string {
  if (status >= 500) return "text-red-600 dark:text-red-400";
  if (status >= 400) return "text-amber-600 dark:text-amber-400";
  if (status >= 300) return "text-fd-muted-foreground";
  return "text-[var(--tt-live-text)]";
}

/**
 * Requests arriving in the inspector, one every second or so while the card is on screen.
 * Server HTML, no JavaScript and Reduce Motion show a still list.
 */
export function RequestStream({ host = "app.teispace.com" }: { host?: string }) {
  const ref = useRef<HTMLElement>(null);
  const [next, setNext] = useState(ROWS);
  const [count, setCount] = useState(1284);

  useEffect(() => {
    const element = ref.current;
    if (
      !element ||
      !("IntersectionObserver" in window) ||
      matchMedia("(prefers-reduced-motion: reduce)").matches
    )
      return;
    let timer = 0;
    const observer = new IntersectionObserver(
      ([entry]) => {
        window.clearInterval(timer);
        if (!entry?.isIntersecting) return;
        timer = window.setInterval(() => {
          setNext((n) => n + 1);
          setCount((n) => n + 1);
        }, 1100);
      },
      { threshold: 0.2 },
    );
    observer.observe(element);
    return () => {
      observer.disconnect();
      window.clearInterval(timer);
    };
  }, []);

  // The newest first; each row keeps its key so only the new one animates in.
  const rows = Array.from({ length: ROWS }, (_, i) => next - 1 - i).filter((n) => n >= 0);

  return (
    <figure ref={ref} className={`${card} p-0`} aria-label={`Requests to ${host} arriving`}>
      <div className="flex items-center justify-between gap-3 border-b border-fd-border px-4 py-2.5">
        <span className="flex min-w-0 items-center gap-2 font-medium">
          <span className="tt-live-dot size-2 shrink-0 rounded-full bg-[var(--tt-live)]" />
          <span className="truncate">{host}</span>
        </span>
        <span className="shrink-0 font-mono text-xs text-fd-muted-foreground tabular-nums">
          {count.toLocaleString("en-US")} requests
        </span>
      </div>
      <ol className="flex flex-col overflow-hidden px-2 py-1.5 font-mono text-xs" aria-hidden>
        {rows.map((n, index) => {
          const request = traffic[n % traffic.length] as Request;
          return (
            <li
              key={n}
              className={`grid grid-cols-[3.75rem_1fr_2.25rem_3.5rem] items-center gap-2 rounded-md px-2 py-1.5 ${index === 0 ? "tt-row-in" : ""}`}
            >
              <span className="text-fd-muted-foreground">{request.method}</span>
              <span className="truncate">{request.path}</span>
              <span className={`text-right ${statusColor(request.status)}`}>{request.status}</span>
              <span className="text-right text-fd-muted-foreground tabular-nums">
                {request.ms >= 1000 ? `${(request.ms / 1000).toFixed(1)} s` : `${request.ms} ms`}
              </span>
            </li>
          );
        })}
      </ol>
    </figure>
  );
}

type Replay = "idle" | "sending" | "done";

/** A captured Stripe webhook replayed with a fresh signature. */
export function ReplayDemo() {
  const ref = useRef<HTMLElement>(null);
  const [state, setState] = useState<Replay>("done");

  usePlayOnView(ref, () => {
    setState("idle");
    const timers = [
      window.setTimeout(() => setState("sending"), 1500),
      window.setTimeout(() => setState("done"), 2700),
    ];
    return () => {
      for (const t of timers) window.clearTimeout(t);
    };
  });

  return (
    <figure ref={ref} className={card} aria-label="Replaying a Stripe webhook">
      <div className="flex items-center justify-between gap-3">
        <span className="min-w-0 truncate font-mono text-xs">
          <span className="text-fd-muted-foreground">POST</span> /webhooks/stripe
        </span>
        <span className="shrink-0 text-xs text-fd-muted-foreground">Stripe webhook</span>
      </div>
      <div className="mt-3 flex items-center gap-2 text-xs">
        <span className="inline-flex h-7 items-center gap-1.5 rounded-full bg-fd-foreground px-3 font-medium text-fd-background">
          {state === "sending" ? (
            <LoaderCircle className="size-3.5 animate-spin" aria-hidden />
          ) : (
            <RotateCcw className="size-3.5" aria-hidden />
          )}
          {state === "sending" ? "Replaying…" : "Replay"}
        </span>
        <span className="inline-flex h-7 items-center rounded-full border border-fd-border px-3 text-fd-muted-foreground">
          Re-sign
        </span>
      </div>
      <p
        aria-live="polite"
        className={`mt-3 flex items-center gap-2 text-xs transition-opacity duration-500 ${state === "done" ? "opacity-100" : "opacity-0"}`}
      >
        <span className="font-mono font-medium text-[var(--tt-live-text)]">200 OK</span>
        <span className="text-fd-muted-foreground">· 49 ms ·</span>
        <span className="inline-flex items-center gap-1 text-fd-muted-foreground">
          <ShieldCheck className="size-3.5 text-[var(--tt-live-text)]" aria-hidden /> signature
          valid
        </span>
      </p>
    </figure>
  );
}

type Answer = "asking" | "allowing" | "applied";

/** An AI agent's change waiting for approval in the app, then applied. */
export function ApprovalDemo() {
  const ref = useRef<HTMLElement>(null);
  const [state, setState] = useState<Answer>("applied");

  usePlayOnView(ref, () => {
    setState("asking");
    const timers = [
      window.setTimeout(() => setState("allowing"), 2200),
      window.setTimeout(() => setState("applied"), 3200),
    ];
    return () => {
      for (const t of timers) window.clearTimeout(t);
    };
  });

  return (
    <figure ref={ref} className={card} aria-label="An AI agent asking to change a route">
      <div className="flex gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-fd-foreground text-fd-background">
          <Logo className="size-5" />
        </span>
        <div className="min-w-0 flex-1">
          {/* core.control.apply_other in locales/en.json */}
          <p className="font-medium">
            Claude Code wants to change your routes in Cloudflare (2 steps):
          </p>
          <ul className="mt-2 space-y-1 text-xs text-fd-muted-foreground">
            <li>Update tunnel “MacBook-Pro” to serve 6 routes</li>
            <li>Add DNS record api.teispace.com → tunnel “MacBook-Pro”</li>
          </ul>
          <div
            className="mt-3 flex min-h-7 items-center justify-end gap-2 text-xs"
            aria-live="polite"
          >
            {state === "applied" ? (
              <span className="mr-auto inline-flex items-center gap-1.5 text-fd-muted-foreground">
                <Check className="size-4 text-[var(--tt-live-text)]" aria-hidden /> Applied · in
                Activity as Claude Code
              </span>
            ) : (
              <>
                <span className="inline-flex h-7 items-center rounded-full border border-fd-border px-3">
                  Deny
                </span>
                <span
                  className={`inline-flex h-7 items-center gap-1.5 rounded-full bg-[var(--tt-accent-fill)] px-3 font-medium text-white transition-transform duration-200 ${state === "allowing" ? "scale-95" : ""}`}
                >
                  {state === "allowing" ? (
                    <LoaderCircle className="size-3.5 animate-spin" aria-hidden />
                  ) : null}
                  Allow
                </span>
              </>
            )}
          </div>
        </div>
      </div>
    </figure>
  );
}

/** A reviewer's comment pinned on a Snapshot, answered a moment later. */
export function CommentDemo() {
  const ref = useRef<HTMLElement>(null);
  const [replied, setReplied] = useState(true);

  usePlayOnView(ref, () => {
    setReplied(false);
    const timer = window.setTimeout(() => setReplied(true), 1800);
    return () => window.clearTimeout(timer);
  });

  return (
    <figure ref={ref} className={card} aria-label="A comment on a Snapshot and its reply">
      <div className="flex items-center justify-between gap-3">
        <span className="flex items-center gap-2">
          <span className="flex size-6 items-center justify-center rounded-full bg-[var(--tt-accent-fill)] text-xs font-semibold text-white">
            1
          </span>
          <span className="font-mono text-xs text-fd-muted-foreground">/pricing</span>
        </span>
        <span className="text-xs text-fd-muted-foreground">preview.teispace.com</span>
      </div>
      <div className="mt-3 space-y-2.5 text-[13px]">
        <p>
          <span className="font-medium">Teispace Design</span>{" "}
          <span className="text-fd-muted-foreground">
            The yearly price should show the discount next to it.
          </span>
        </p>
        <p
          aria-live="polite"
          className={`transition-[opacity,transform] duration-500 ${replied ? "translate-y-0 opacity-100" : "translate-y-1 opacity-0"}`}
        >
          <span className="font-medium">Teispace</span>{" "}
          <span className="rounded bg-fd-accent px-1 text-[11px] text-fd-muted-foreground">
            Owner
          </span>{" "}
          <span className="text-fd-muted-foreground">Good catch, adding it now.</span>
        </p>
      </div>
    </figure>
  );
}

type Scan = "checking" | "found";

/** The exposure check finding a served .env file before a share goes public. */
export function ExposureDemo() {
  const ref = useRef<HTMLElement>(null);
  const [state, setState] = useState<Scan>("found");

  usePlayOnView(ref, () => {
    setState("checking");
    const timer = window.setTimeout(() => setState("found"), 1600);
    return () => window.clearTimeout(timer);
  });

  return (
    <figure ref={ref} className={card} aria-label="The exposure check before sharing">
      <p className="flex items-center gap-2 font-medium">
        {state === "checking" ? (
          <>
            <LoaderCircle className="size-4 animate-spin text-fd-muted-foreground" aria-hidden />
            Checking localhost:4000…
          </>
        ) : (
          <>
            <span className="rounded bg-red-500/12 px-1.5 py-0.5 text-[11px] font-semibold text-red-700 uppercase dark:text-red-300">
              High
            </span>
            The .env file is served
          </>
        )}
      </p>
      <div
        aria-live="polite"
        className={`transition-opacity duration-500 ${state === "found" ? "opacity-100" : "opacity-0"}`}
      >
        <p className="mt-2 text-xs text-fd-muted-foreground">
          /.env answers with APP_KEY, DATABASE_URL and STRIPE_SECRET_KEY. Only the names are shown,
          never the values.
        </p>
        <div className="mt-3 flex justify-end gap-2 text-xs">
          <span className="inline-flex h-7 items-center rounded-full border border-fd-border px-3">
            Share Anyway
          </span>
          <span className="inline-flex h-7 items-center rounded-full bg-fd-foreground px-3 font-medium text-fd-background">
            Cancel
          </span>
        </div>
      </div>
    </figure>
  );
}
