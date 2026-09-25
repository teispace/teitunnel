"use client";

import { Check, Copy, LoaderCircle, QrCode, TriangleAlert } from "lucide-react";
import { type RefObject, useEffect, useRef, useState } from "react";

/**
 * Runs `play` once when the element is half in view. Without it (Reduce Motion, no
 * IntersectionObserver, server HTML) the demo stays in its finished state.
 */
export function usePlayOnView(ref: RefObject<HTMLElement | null>, play: () => () => void) {
  // The latest `play`, so re-renders (every tick of a demo) don't restart it.
  const latest = useRef(play);
  latest.current = play;
  useEffect(() => {
    const element = ref.current;
    if (
      !element ||
      !("IntersectionObserver" in window) ||
      matchMedia("(prefers-reduced-motion: reduce)").matches
    )
      return;
    let stop: (() => void) | null = null;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) return;
        observer.disconnect();
        stop = latest.current();
      },
      { threshold: 0.5 },
    );
    observer.observe(element);
    return () => {
      observer.disconnect();
      stop?.();
    };
  }, [ref]);
}

export const card =
  "w-full min-w-0 max-w-sm rounded-2xl border border-fd-border bg-fd-background p-4 text-sm shadow-2xl shadow-black/15 dark:shadow-black/50";

const shareUrl = "https://quiet-river-lamp-orbit.trycloudflare.com";

/** A Quick Share starting: waiting for Cloudflare, then the URL and live requests. */
export function QuickShareDemo() {
  const ref = useRef<HTMLElement>(null);
  // Server HTML and Reduce Motion show a running share.
  const [live, setLive] = useState(true);
  const [requests, setRequests] = useState(128);
  const [bars, setBars] = useState<number[]>([4, 7, 5, 9, 6, 11, 8, 12, 9, 14, 10, 13]);

  usePlayOnView(ref, () => {
    setLive(false);
    setRequests(0);
    setBars(Array(12).fill(1));
    const timers: number[] = [];
    let ticker = 0;
    let ticks = 0;
    timers.push(
      window.setTimeout(() => {
        setLive(true);
        ticker = window.setInterval(() => {
          // A minute of traffic is enough; then the chart rests.
          if (++ticks > 90) window.clearInterval(ticker);
          const burst = 1 + Math.floor(Math.random() * 4);
          setRequests((n) => n + burst);
          setBars((b) => [...b.slice(1), 2 + burst * 3]);
        }, 650);
      }, 1600),
    );
    return () => {
      for (const t of timers) window.clearTimeout(t);
      window.clearInterval(ticker);
    };
  });

  return (
    <figure ref={ref} className={card} aria-label="A Quick Share of a Vite dev server">
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-2 font-medium">
          <span
            className={`size-2 rounded-full ${live ? "tt-live-dot bg-[var(--tt-live)]" : "bg-amber-500"}`}
          />
          {live ? "Live" : "Getting a URL…"}
        </span>
        <span className="font-mono text-xs text-fd-muted-foreground">Vite · :5173</span>
      </div>
      <div className="mt-3 flex items-center gap-2 rounded-xl border border-fd-border bg-fd-card px-3 py-2">
        {live ? (
          <span className="min-w-0 flex-1 truncate font-mono text-xs">{shareUrl}</span>
        ) : (
          <span className="flex flex-1 items-center gap-2 text-xs text-fd-muted-foreground">
            <LoaderCircle className="size-3.5 animate-spin" aria-hidden /> Waiting for Cloudflare…
          </span>
        )}
        <Copy className="size-3.5 shrink-0 text-fd-muted-foreground" aria-hidden />
        <QrCode className="size-3.5 shrink-0 text-fd-muted-foreground" aria-hidden />
      </div>
      <div className="mt-3 flex items-end justify-between gap-4">
        <span className="text-xs text-fd-muted-foreground">
          <span className="font-mono text-base font-medium text-fd-foreground tabular-nums">
            {requests}
          </span>{" "}
          requests · 0 errors
        </span>
        <span className="flex h-8 shrink-0 items-end gap-0.5" aria-hidden>
          {bars.map((height, index) => (
            <span
              // biome-ignore lint/suspicious/noArrayIndexKey: a fixed-length chart
              key={index}
              className="w-1.5 rounded-sm bg-[var(--tt-accent)] transition-[height] duration-500"
              style={{ height: `${Math.min(100, height * 7)}%`, opacity: 0.35 + index / 18 }}
            />
          ))}
        </span>
      </div>
    </figure>
  );
}

type Fix = "problem" | "fixing" | "fixed";

/** The Doctor finding a missing DNS record, and fixing it. */
export function DoctorDemo() {
  const ref = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<Fix>("fixed");

  usePlayOnView(ref, () => {
    setState("problem");
    const timers = [
      window.setTimeout(() => setState("fixing"), 1800),
      window.setTimeout(() => setState("fixed"), 3300),
    ];
    return () => {
      for (const t of timers) window.clearTimeout(t);
    };
  });

  return (
    <div ref={ref} className={card} aria-live="polite">
      <div className="flex gap-3">
        <span className="mt-0.5">
          {state === "fixed" ? (
            <Check className="size-4.5 text-[var(--tt-live)]" aria-hidden />
          ) : (
            <TriangleAlert className="size-4.5 text-amber-500" aria-hidden />
          )}
        </span>
        <div className="min-w-0 flex-1">
          <p className="font-medium">
            {state === "fixed"
              ? "docs.teispace.com is live"
              : "docs.teispace.com has no DNS record"}
          </p>
          <p className="mt-1 text-xs text-fd-muted-foreground">
            {state === "fixed"
              ? "Added a proxied CNAME to this Mac's tunnel and checked it through Cloudflare."
              : "The tunnel serves this hostname, but nothing points it at the tunnel, so it doesn't resolve."}
          </p>
          <div className="mt-3 flex items-center gap-2">
            <span
              className={`inline-flex h-7 items-center gap-1.5 rounded-full px-3 text-xs font-medium transition-colors ${
                state === "fixed"
                  ? "bg-fd-accent text-fd-muted-foreground"
                  : "bg-fd-foreground text-fd-background"
              }`}
            >
              {state === "fixing" ? (
                <>
                  <LoaderCircle className="size-3.5 animate-spin" aria-hidden /> Fixing…
                </>
              ) : state === "fixed" ? (
                "Fixed"
              ) : (
                "Fix the DNS Record"
              )}
            </span>
            <span className="text-xs text-fd-muted-foreground">
              {state === "fixed" ? "Undo in Activity" : "dns.missing"}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
