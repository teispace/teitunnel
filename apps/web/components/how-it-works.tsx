"use client";

import { Check, Download, Globe, LoaderCircle, LogIn, RotateCcw } from "lucide-react";
import {
  type CSSProperties,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";

/** Seconds each step plays, and between steps while the line draws. */
const STEP = 3.2;
const GAP = 0.5;
const TOTAL = STEP * 3 + GAP * 2;
/** How much faster than real life the clock runs. */
const SPEED = 18;

/** How far step `index` has played (0–1) at `time`, and the line into the next one. */
function progress(time: number, index: number) {
  const start = index * (STEP + GAP);
  const step = Math.min(1, Math.max(0, (time - start) / STEP));
  const link = Math.min(1, Math.max(0, (time - start - STEP) / GAP));
  return { step, link };
}

function Tick({ done, children }: { done: boolean; children: ReactNode }) {
  return (
    <span className="tt-how-line" data-done={done ? "" : undefined}>
      <span className="tt-how-tick" aria-hidden>
        {done ? <Check className="size-3" /> : <span className="tt-how-dot" />}
      </span>
      {children}
    </span>
  );
}

function Install({ p }: { p: number }) {
  const downloaded = Math.min(1, p / 0.62);
  const verified = p > 0.86;
  return (
    <div className="tt-how-stage">
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="flex min-w-0 items-center gap-2">
          <Download className="size-3.5 shrink-0 text-fd-muted-foreground" aria-hidden />
          <span className="truncate font-mono">Teitunnel.dmg</span>
        </span>
        <span className="shrink-0 font-mono text-fd-muted-foreground tabular-nums">
          {(downloaded * 24.9).toFixed(1)} / 24.9 MB
        </span>
      </div>
      <span className="tt-how-bar" aria-hidden>
        <span style={{ transform: `scaleX(${downloaded})` }} />
      </span>
      <span className="tt-how-line mt-3" data-done={verified ? "" : undefined}>
        <span className="tt-how-tick" aria-hidden>
          {verified ? (
            <Check className="size-3" />
          ) : p > 0.64 ? (
            <LoaderCircle className="size-3 animate-spin" />
          ) : (
            <span className="tt-how-dot" />
          )}
        </span>
        <span className="font-mono text-xs">
          cloudflared {verified ? "verified" : p > 0.64 ? "verifying…" : "waiting"}
        </span>
      </span>
    </div>
  );
}

function Connect({ p }: { p: number }) {
  const pressed = p > 0.14;
  return (
    <div className="tt-how-stage">
      <span className="tt-how-button" data-pressed={pressed ? "" : undefined}>
        {pressed && p < 0.3 ? (
          <LoaderCircle className="size-3.5 animate-spin" aria-hidden />
        ) : (
          <LogIn className="size-3.5" aria-hidden />
        )}
        Sign in with Cloudflare
      </span>
      <span className="mt-3 flex flex-wrap gap-1.5">
        {(
          [
            ["Tunnels", 0.38],
            ["DNS", 0.56],
            ["Access", 0.74],
          ] as const
        ).map(([name, at]) => (
          <span key={name} className="tt-how-chip" data-on={p > at ? "" : undefined}>
            <Check className="size-3" aria-hidden /> {name}
          </span>
        ))}
      </span>
      <span className="mt-3 block text-xs text-fd-muted-foreground">
        Or share right away, with no account.
      </span>
    </div>
  );
}

function Apply({ p }: { p: number }) {
  return (
    <div className="tt-how-stage">
      <span className="flex flex-col gap-1.5">
        <Tick done={p > 0.22}>Create tunnel “web-01”</Tick>
        <Tick done={p > 0.44}>Route app.teispace.com → :3000</Tick>
        <Tick done={p > 0.64}>Add its DNS record</Tick>
      </span>
      <span className="tt-how-live" data-on={p > 0.84 ? "" : undefined}>
        <Globe className="size-3.5" aria-hidden />
        <span className="font-mono">app.teispace.com</span>
        <span className="tt-how-live-dot tt-live-dot" aria-hidden />
        Live
      </span>
    </div>
  );
}

const steps = [
  {
    title: "Install",
    body: "Download the app for macOS, Windows or Linux. It fetches and verifies cloudflared for you.",
    Stage: Install,
  },
  {
    title: "Share or connect",
    body: "Share a port right away with no account, or sign in to Cloudflare and grant only the permissions you need.",
    Stage: Connect,
  },
  {
    title: "Review and apply",
    body: "Add a route, read exactly what will change, and apply. Teitunnel checks the URL works when it's done.",
    Stage: Apply,
  },
];

function clock(seconds: number): string {
  const whole = Math.floor(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

/**
 * How it works, timed: when the section comes into view the three steps play one after
 * another, each handing over to the next along a line that draws itself, while a clock
 * counts from a fresh install to a live address (sped up, and it says so). Replay runs it
 * again. Without motion everything shows as done.
 */
export function HowItWorks() {
  const ref = useRef<HTMLDivElement>(null);
  const [time, setTime] = useState(TOTAL);
  const [playing, setPlaying] = useState(false);
  const frame = useRef(0);

  const play = useCallback(() => {
    cancelAnimationFrame(frame.current);
    const start = performance.now();
    setPlaying(true);
    const tick = (now: number) => {
      const elapsed = (now - start) / 1000;
      setTime(Math.min(TOTAL, elapsed));
      if (elapsed < TOTAL) frame.current = requestAnimationFrame(tick);
      else setPlaying(false);
    };
    frame.current = requestAnimationFrame(tick);
  }, []);

  useEffect(() => {
    const element = ref.current;
    if (
      !element ||
      !document.documentElement.classList.contains("tt-motion") ||
      !("IntersectionObserver" in window)
    )
      return;
    setTime(0);
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) return;
        observer.disconnect();
        play();
      },
      { threshold: 0.45 },
    );
    observer.observe(element);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame.current);
    };
  }, [play]);

  const done = time >= TOTAL;
  return (
    <div ref={ref} className="tt-how">
      <div className="mb-8 flex flex-wrap items-center gap-3">
        <span className="tt-how-clock" data-done={done ? "" : undefined}>
          <span className="tt-how-clock-dot" aria-hidden />
          <span className="font-mono text-base tabular-nums">{clock(time * SPEED)}</span>
          <span className="text-xs text-fd-muted-foreground">
            {done ? "from download to a live address" : "fresh install, sped up"}
          </span>
        </span>
        <button
          type="button"
          onClick={play}
          disabled={playing}
          className="inline-flex h-9 items-center gap-1.5 rounded-full border border-fd-border px-3.5 text-sm transition-colors hover:bg-fd-accent disabled:opacity-40"
        >
          <RotateCcw className="size-3.5" aria-hidden /> Replay
        </button>
      </div>
      <ol className="tt-how-steps">
        {steps.map(({ title, body, Stage }, index) => {
          const { step, link } = progress(time, index);
          return (
            <li
              key={title}
              className="tt-how-card"
              data-state={step >= 1 ? "done" : step > 0 ? "active" : "waiting"}
              style={{ "--link": link } as CSSProperties}
            >
              <span className="tt-how-link" aria-hidden />
              <span className="tt-how-num">
                {step >= 1 ? <Check className="size-4" aria-hidden /> : index + 1}
              </span>
              <h3 className="mt-4 font-medium">{title}</h3>
              <p className="mt-1 text-sm text-fd-muted-foreground">{body}</p>
              <Stage p={step} />
            </li>
          );
        })}
      </ol>
    </div>
  );
}
