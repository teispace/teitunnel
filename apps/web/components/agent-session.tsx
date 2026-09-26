"use client";

import { Check, LoaderCircle, RotateCcw, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Logo } from "./logo";

/** A line of the session: what the person asked, a Teitunnel tool the agent called, or its reply. */
type Line =
  | { kind: "user"; text: string }
  | { kind: "tool"; name: string; result: string }
  | { kind: "agent"; text: string };

// Tool names and wording as the MCP server has them (crates/mcp, locales/en.json).
const before: Line[] = [
  { kind: "user", text: "Put my API on api.yourhost.com" },
  { kind: "tool", name: "list_local_services", result: "api · localhost:8080" },
  {
    kind: "tool",
    name: "plan_change",
    result: "2 steps: update tunnel “MacBook-Pro”, add a DNS record",
  },
];
const asking: Line = {
  kind: "tool",
  name: "apply_plan",
  result: "Waiting for your approval in Teitunnel…",
};
const outcomes: Record<"allowed" | "denied", Line[]> = {
  allowed: [
    { kind: "tool", name: "apply_plan", result: "Applied 2 steps · in Activity as Claude Code" },
    { kind: "agent", text: "Done: https://api.yourhost.com works." },
  ],
  denied: [
    { kind: "tool", name: "apply_plan", result: "Declined in Teitunnel" },
    { kind: "agent", text: "Okay, I didn't change anything." },
  ],
};

/** Milliseconds between lines, and how long the question waits before it's answered for you. */
const PACE = 900;
const AUTO_ALLOW = 6500;

type Phase = "typing" | "asking" | "allowed" | "denied";

function Row({ line }: { line: Line }) {
  if (line.kind === "user") {
    return (
      <p className="tt-agent-line flex gap-2">
        <span className="text-[var(--tt-accent-text)]" aria-hidden>
          ›
        </span>
        <span className="text-fd-foreground">{line.text}</span>
      </p>
    );
  }
  if (line.kind === "agent") {
    return <p className="tt-agent-line text-fd-foreground">{line.text}</p>;
  }
  return (
    <p className="tt-agent-line">
      <span className="text-fd-muted-foreground">teitunnel · </span>
      <span className="text-fd-foreground">{line.name}</span>
      <span className="block ps-4 text-fd-muted-foreground">⎿ {line.result}</span>
    </p>
  );
}

/**
 * An agent at work through Teitunnel's MCP server: it finds the service, plans the change
 * and asks to apply it, and the change waits for the reader's own Allow or Deny (answered
 * for them after a few seconds). Replay runs it again. Without motion it shows the change
 * allowed.
 */
export function AgentSession() {
  const ref = useRef<HTMLDivElement>(null);
  const [shown, setShown] = useState(before.length + 1);
  const [phase, setPhase] = useState<Phase>("allowed");
  const timers = useRef<number[]>([]);

  const clear = useCallback(() => {
    for (const timer of timers.current) window.clearTimeout(timer);
    timers.current = [];
  }, []);

  const answer = useCallback(
    (allow: boolean) => {
      clear();
      setPhase(allow ? "allowed" : "denied");
    },
    [clear],
  );

  const play = useCallback(() => {
    clear();
    setPhase("typing");
    setShown(0);
    const steps = before.length + 1;
    for (let i = 1; i <= steps; i++) {
      timers.current.push(window.setTimeout(() => setShown(i), 400 + i * PACE));
    }
    const ask = 400 + steps * PACE + 300;
    timers.current.push(
      window.setTimeout(() => setPhase("asking"), ask),
      window.setTimeout(() => setPhase("allowed"), ask + AUTO_ALLOW),
    );
  }, [clear]);

  useEffect(() => {
    const element = ref.current;
    if (
      !element ||
      !document.documentElement.classList.contains("tt-motion") ||
      !("IntersectionObserver" in window)
    )
      return;
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
      clear();
    };
  }, [play, clear]);

  const answered = phase === "allowed" || phase === "denied";
  const lines = [
    ...before.slice(0, shown),
    ...(shown > before.length && !answered ? [asking] : []),
    ...(answered ? outcomes[phase] : []),
  ];

  return (
    <div ref={ref} className="tt-agent" data-phase={phase}>
      <div className="tt-agent-window tt-window">
        <div className="flex items-center gap-2 border-b border-fd-border px-4 py-2.5 text-xs text-fd-muted-foreground">
          <span className="flex gap-1.5" aria-hidden>
            <span className="size-2.5 rounded-full bg-fd-border" />
            <span className="size-2.5 rounded-full bg-fd-border" />
            <span className="size-2.5 rounded-full bg-fd-border" />
          </span>
          <span className="mx-auto font-mono">claude — ~/shop</span>
          <button
            type="button"
            onClick={play}
            disabled={!answered}
            className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 transition-colors hover:bg-fd-accent hover:text-fd-foreground disabled:opacity-0"
          >
            <RotateCcw className="size-3" aria-hidden /> Replay
          </button>
        </div>
        <div
          className="flex min-h-[19rem] flex-col gap-3 p-4 font-mono text-[12.5px] leading-relaxed sm:p-5"
          aria-live="polite"
        >
          {lines.map((line, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: lines only ever append in order
            <Row key={`${phase === "denied" ? "d" : "a"}-${index}`} line={line} />
          ))}
          {phase === "typing" ? (
            <span className="tt-caret inline-block h-4 w-2 bg-fd-muted-foreground/60" aria-hidden />
          ) : null}
        </div>
      </div>

      {/* The question, as the app asks it (core.control.apply_other in locales/en.json). */}
      <div className="tt-agent-ask" data-open={phase === "asking" ? "" : undefined}>
        <div className="flex gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-fd-foreground text-fd-background">
            <Logo className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="font-medium">
              Claude Code wants to change your routes in Cloudflare (2 steps):
            </p>
            <ul className="mt-2 space-y-1 text-xs text-fd-muted-foreground">
              <li>Update tunnel “MacBook-Pro” to serve 6 routes</li>
              <li>Add DNS record api.yourhost.com → tunnel “MacBook-Pro”</li>
            </ul>
            <div className="mt-3 flex items-center justify-end gap-2 text-xs">
              <button
                type="button"
                tabIndex={phase === "asking" ? 0 : -1}
                onClick={() => answer(false)}
                className="inline-flex h-7 items-center gap-1 rounded-full border border-fd-border px-3 transition-colors hover:bg-fd-accent"
              >
                <X className="size-3.5" aria-hidden /> Deny
              </button>
              <button
                type="button"
                tabIndex={phase === "asking" ? 0 : -1}
                onClick={() => answer(true)}
                className="tt-agent-allow inline-flex h-7 items-center gap-1 rounded-full bg-[var(--tt-accent-fill)] px-3 font-medium text-white transition-transform active:scale-95"
              >
                <Check className="size-3.5" aria-hidden /> Allow
              </button>
            </div>
            <p className="mt-2 flex items-center gap-1.5 text-[11px] text-fd-muted-foreground">
              <LoaderCircle className="size-3 animate-spin" aria-hidden /> Your answer, not the
              agent's
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
