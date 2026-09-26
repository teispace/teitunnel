"use client";

import { Copy, Laptop } from "lucide-react";
import { useEffect, useRef, useState } from "react";

// Words like the ones in Quick Share addresses (four random words at trycloudflare.com).
const words = [
  "quiet",
  "river",
  "lamp",
  "orbit",
  "amber",
  "field",
  "cloud",
  "note",
  "mellow",
  "stone",
  "paper",
  "kite",
  "silver",
  "harbor",
  "maple",
  "signal",
];

function address(seed: number): string {
  const pick = (i: number) => words[(seed * 7 + i * 5) % words.length];
  return `${pick(0)}-${pick(1)}-${pick(2)}-${pick(3)}.trycloudflare.com`;
}

type Phase = "sending" | "live";

/**
 * A Quick Share in miniature: a request leaves localhost, travels the line, and a public
 * address appears, live; then again with a new address. Still (and live) without motion.
 */
export function ShareLoop() {
  const ref = useRef<HTMLDivElement>(null);
  const [round, setRound] = useState(0);
  const [phase, setPhase] = useState<Phase>("live");

  useEffect(() => {
    const element = ref.current;
    if (
      !element ||
      !document.documentElement.classList.contains("tt-motion") ||
      !("IntersectionObserver" in window)
    )
      return;
    let timers: number[] = [];
    const play = () => {
      setPhase("sending");
      timers.push(
        window.setTimeout(() => setPhase("live"), 1300),
        window.setTimeout(() => {
          setRound((r) => r + 1);
          play();
        }, 4800),
      );
    };
    const observer = new IntersectionObserver(([entry]) => {
      for (const timer of timers) window.clearTimeout(timer);
      timers = [];
      if (entry?.isIntersecting) play();
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
      for (const timer of timers) window.clearTimeout(timer);
    };
  }, []);

  return (
    <div
      ref={ref}
      className="tt-share-loop"
      data-phase={phase}
      role="img"
      aria-label="A Quick Share: localhost:5173 gets a public trycloudflare.com address"
    >
      <span className="tt-share-end">
        <Laptop className="size-4 text-fd-muted-foreground" aria-hidden />
        <span className="font-mono text-sm">localhost:5173</span>
      </span>
      <span className="tt-share-line" aria-hidden>
        <span key={round} className="tt-share-dot" />
      </span>
      <span className="tt-share-end tt-share-url">
        <span className="tt-share-live" aria-hidden />
        <span key={round} className="tt-share-address min-w-0 truncate font-mono text-sm">
          {address(round)}
        </span>
        <Copy className="size-3.5 shrink-0 text-fd-muted-foreground" aria-hidden />
      </span>
    </div>
  );
}
