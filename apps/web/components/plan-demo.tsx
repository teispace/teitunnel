"use client";

import { Check, Circle, LoaderCircle, RotateCcw } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

// The steps as the app words them (locales/en.json, plan.step).
const steps = [
  { text: "Create tunnel “MacBook-Pro”", detail: "the first route in this account" },
  {
    text: "Update tunnel “MacBook-Pro” to serve 1 route",
    detail: "app.example.com → http://localhost:3000",
  },
  {
    text: "Add DNS record app.example.com → tunnel “MacBook-Pro”",
    detail: "proxied CNAME to <id>.cfargotunnel.com",
  },
  { text: "Check https://app.example.com works", detail: "through Cloudflare's edge" },
];

type State = "waiting" | "working" | "done";

/** A plan being applied, step by step, when it scrolls into view (all done with Reduce Motion). */
export function PlanDemo() {
  const ref = useRef<HTMLDivElement>(null);
  const timers = useRef<number[]>([]);
  // Server HTML and no-JS show the finished plan.
  const [progress, setProgress] = useState(steps.length);

  const play = useCallback(() => {
    for (const timer of timers.current) window.clearTimeout(timer);
    timers.current = [];
    setProgress(0);
    for (let step = 1; step <= steps.length; step++) {
      timers.current.push(window.setTimeout(() => setProgress(step), 350 + step * 750));
    }
  }, []);

  useEffect(() => {
    const element = ref.current;
    if (!element || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    setProgress(0);
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) return;
        observer.disconnect();
        play();
      },
      { threshold: 0.5 },
    );
    observer.observe(element);
    return () => {
      observer.disconnect();
      for (const timer of timers.current) window.clearTimeout(timer);
    };
  }, [play]);

  const state = (index: number): State =>
    index < progress ? "done" : index === progress ? "working" : "waiting";
  const finished = progress >= steps.length;

  return (
    <div
      ref={ref}
      className="overflow-hidden rounded-2xl border border-fd-border bg-fd-card shadow-xl shadow-black/5 dark:shadow-black/30"
    >
      <div className="flex items-center justify-between border-b border-fd-border px-5 py-3">
        <p className="text-sm font-medium">Add app.example.com</p>
        <button
          type="button"
          onClick={play}
          className="inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-xs text-fd-muted-foreground transition-colors hover:bg-fd-accent hover:text-fd-foreground"
        >
          <RotateCcw className="size-3.5" aria-hidden /> Replay
        </button>
      </div>
      <ol className="flex flex-col gap-1 p-3" aria-label="Steps of the change">
        {steps.map((step, index) => {
          const current = state(index);
          return (
            <li
              key={step.text}
              data-state={current}
              className="tt-step flex items-start gap-3 rounded-lg px-2 py-2"
            >
              <span className="tt-step-icon mt-0.5 flex size-5 shrink-0 items-center justify-center">
                {current === "done" ? (
                  <Check className="size-4.5" aria-label="Done" />
                ) : current === "working" ? (
                  <LoaderCircle
                    className="size-4.5 animate-spin text-[var(--tt-accent)]"
                    aria-label="In progress"
                  />
                ) : (
                  <Circle className="size-4" aria-label="Waiting" />
                )}
              </span>
              <span className="min-w-0">
                <span className="block text-sm">{step.text}</span>
                <span className="block font-mono text-xs text-fd-muted-foreground">
                  {step.detail}
                </span>
              </span>
            </li>
          );
        })}
      </ol>
      <div
        aria-live="polite"
        className="flex items-center gap-2.5 border-t border-fd-border px-5 py-3 text-sm transition-opacity duration-500"
        style={{ opacity: finished ? 1 : 0.4 }}
      >
        <span
          className={`size-2 rounded-full ${finished ? "tt-live-dot bg-[var(--tt-live)]" : "bg-fd-muted-foreground"}`}
        />
        {finished ? (
          <span>
            <span className="font-medium">Live</span>
            <span className="text-fd-muted-foreground"> · https://app.example.com works</span>
          </span>
        ) : (
          <span className="text-fd-muted-foreground">Applying…</span>
        )}
      </div>
    </div>
  );
}
