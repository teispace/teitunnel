"use client";

import { Check, Globe, Laptop, ScanSearch, ShieldCheck, Smartphone } from "lucide-react";
import { type ComponentType, type CSSProperties, useEffect, useRef } from "react";
import { onSceneProgress } from "./scenes";

interface Stop {
  icon: ComponentType<{ className?: string }>;
  title: string;
  detail: string;
  /** Where it sits along the line, 0–1. */
  at: number;
  /** What it shows while the request is there. */
  note: string;
}

const stops: Stop[] = [
  {
    icon: Smartphone,
    title: "A visitor",
    detail: "app.yourhost.com",
    at: 0,
    note: "GET /api/projects",
  },
  {
    icon: Globe,
    title: "Cloudflare's edge",
    detail: "your account",
    at: 0.32,
    note: "TLS ✓ · login ✓ · bot rules ✓",
  },
  {
    icon: ScanSearch,
    title: "Teitunnel",
    detail: "on your computer",
    at: 0.7,
    note: "Recorded · Authorization masked",
  },
  {
    icon: Laptop,
    title: "Your service",
    detail: "localhost:3000",
    at: 1,
    note: "200 OK · 38 ms",
  },
];

const steps = [
  {
    title: "A visitor opens your address",
    body: "A phone anywhere asks for https://app.yourhost.com. DNS points it at Cloudflare, never at your home or office.",
  },
  {
    title: "Cloudflare's edge decides who gets in",
    body: "HTTPS ends there. Your login, bot rules and rate limits run at the edge, so blocked traffic never reaches your computer.",
  },
  {
    title: "Down the tunnel your computer opened",
    body: "cloudflared dialed out to Cloudflare when you shared, so there's no port to forward and your IP address stays private.",
  },
  {
    title: "Teitunnel's inspector sees it",
    body: "Headers, body and timing are recorded on your computer, with credentials masked. Replay it any time.",
  },
  {
    title: "Your service answers, and back it goes",
    body: "localhost:3000 replies as it would to you, and the answer takes the same way back in milliseconds.",
  },
];

/** Where the request is along the line for scroll progress `p`, and which step that is. */
function position(p: number): { t: number; back: boolean; step: number } {
  const lerp = (from: number, to: number, a: number, b: number) =>
    from + (to - from) * Math.min(1, Math.max(0, (p - a) / (b - a)));
  if (p < 0.16) return { t: lerp(0, 0.32, 0, 0.16), back: false, step: 0 };
  if (p < 0.26) return { t: 0.32, back: false, step: 1 };
  if (p < 0.5) return { t: lerp(0.32, 0.7, 0.26, 0.5), back: false, step: 2 };
  if (p < 0.62) return { t: 0.7, back: false, step: 3 };
  if (p < 0.76) return { t: lerp(0.7, 1, 0.62, 0.76), back: false, step: 3 };
  if (p < 0.82) return { t: 1, back: false, step: 4 };
  return { t: lerp(1, 0, 0.82, 1), back: true, step: 4 };
}

/**
 * How a request reaches your laptop, told by scrolling: the section stays on screen while a
 * request travels from a visitor through Cloudflare's edge and the tunnel to Teitunnel and
 * your service, and the answer comes back. The steps are plain text for everyone; without
 * motion the diagram shows the whole way at once.
 */
export function Journey() {
  const ref = useRef<HTMLElement>(null);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    return onSceneProgress(element, (p) => {
      const { t, back, step } = position(p);
      element.style.setProperty("--t", t.toFixed(4));
      element.dataset.phase = String(step);
      if (back) element.dataset.back = "";
      else delete element.dataset.back;
      // The stop whose note shows: the one this step is about (none in the tunnel).
      element.dataset.at = String([0, 1, -1, 2, 3][step] ?? -1);
    });
  }, []);

  return (
    <section
      ref={ref}
      id="how-it-reaches-you"
      aria-labelledby="journey-title"
      data-scene="pin"
      data-steps="5"
      className="tt-journey relative"
    >
      <div className="tt-journey-stage">
        <div className="mx-auto grid h-full w-full max-w-6xl grid-cols-1 content-center gap-10 px-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-16">
          <div>
            <p className="mb-3 font-mono text-xs uppercase tracking-[0.18em] text-[var(--tt-accent)]">
              How it reaches you
            </p>
            <h2
              id="journey-title"
              className="text-3xl font-semibold tracking-tight text-balance md:text-5xl"
            >
              From a phone anywhere to{" "}
              <span className="text-[var(--tt-accent-text)]">localhost</span>.
            </h2>
            <ol className="tt-journey-steps mt-8 flex flex-col gap-1">
              {steps.map((step, index) => (
                <li key={step.title} data-index={index} className="tt-journey-step">
                  <span className="tt-journey-num">{index + 1}</span>
                  <span>
                    <span className="block font-medium">{step.title}</span>
                    <span className="tt-journey-body block text-sm text-fd-muted-foreground">
                      <span className="block">{step.body}</span>
                    </span>
                  </span>
                </li>
              ))}
            </ol>
          </div>

          <figure
            aria-label="A request goes from a visitor to Cloudflare's edge, through the tunnel your computer opened, to Teitunnel and your service, and the answer comes back"
            className="tt-journey-map relative"
          >
            <div className="tt-journey-line" aria-hidden>
              <span className="tt-journey-tunnel">
                <span className="tt-journey-tunnel-label">outbound tunnel</span>
              </span>
              <span className="tt-journey-fill" />
              <span className="tt-journey-packet" />
            </div>
            <ol className="tt-journey-stops">
              {stops.map(({ icon: Icon, title, detail, at, note }, index) => (
                <li
                  key={title}
                  data-index={index}
                  className="tt-journey-stop"
                  style={{ "--at": at } as CSSProperties}
                >
                  <span className="tt-journey-node">
                    <Icon className="size-5" aria-hidden />
                    {index === 1 ? (
                      <ShieldCheck className="tt-journey-shield size-3.5" aria-hidden />
                    ) : null}
                  </span>
                  <span className="tt-journey-label">
                    <span className="block text-sm font-medium">{title}</span>
                    <span className="block font-mono text-xs text-fd-muted-foreground">
                      {detail}
                    </span>
                  </span>
                  <span className="tt-journey-note font-mono" aria-hidden>
                    {index === 3 ? <Check className="size-3" aria-hidden /> : null}
                    {note}
                  </span>
                </li>
              ))}
            </ol>
            <figcaption className="tt-journey-caption text-sm text-fd-muted-foreground">
              No port forwarding, no public IP: the connection starts on your computer.
            </figcaption>
          </figure>
        </div>
      </div>
    </section>
  );
}
