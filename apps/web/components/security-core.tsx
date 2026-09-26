"use client";

import { type CSSProperties, type ReactNode, useEffect, useRef, useState } from "react";

export interface Pledge {
  /** Its icon, as an element (a server component can't pass a component to this one). */
  icon: ReactNode;
  title: string;
  body: string;
}

interface Trace {
  d: string;
  /** Where it starts, when a dot marks it. */
  dot?: { x: number; y: number };
  /** Where it meets the core: a pin. */
  pin: { x: number; y: number; side: "left" | "right" | "top" | "bottom" };
}

/** A pulse of data on its way along a trace into the core. */
interface Pulse {
  id: number;
  trace: number;
  duration: number;
}

/** An orthogonal path through `points`, with rounded corners. */
function rounded(points: { x: number; y: number }[], radius = 12): string {
  const [first, ...rest] = points;
  if (!first) return "";
  let d = `M${first.x},${first.y}`;
  for (let i = 0; i < rest.length; i++) {
    const here = rest[i];
    const after = rest[i + 1];
    const before = i === 0 ? first : rest[i - 1];
    if (!here || !before) continue;
    if (!after) {
      d += ` L${here.x},${here.y}`;
      continue;
    }
    const inLength = Math.hypot(here.x - before.x, here.y - before.y);
    const outLength = Math.hypot(after.x - here.x, after.y - here.y);
    const r = Math.min(radius, inLength / 2, outLength / 2);
    const ix = here.x - (Math.sign(here.x - before.x) * r || 0);
    const iy = here.y - (Math.sign(here.y - before.y) * r || 0);
    const ox = here.x + (Math.sign(after.x - here.x) * r || 0);
    const oy = here.y + (Math.sign(after.y - here.y) * r || 0);
    d += ` L${ix},${iy} Q${here.x},${here.y} ${ox},${oy}`;
  }
  return d;
}

/**
 * Security and privacy as a core: the promise in the middle, styled like a chip, and each
 * of the six promises around it wired to it, with data flowing along every trace into the
 * core. The traces are measured from the cards, so they follow the layout; on small screens
 * the cards stack under the core without traces. Without motion the traces stay still.
 */
export function SecurityCore({ pledges, children }: { pledges: Pledge[]; children: ReactNode }) {
  const gridRef = useRef<HTMLDivElement>(null);
  const coreRef = useRef<HTMLDivElement>(null);
  const [traces, setTraces] = useState<Trace[]>([]);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [pulses, setPulses] = useState<Pulse[]>([]);

  // Data arrives from any node at any moment: a pulse on a random trace every so often,
  // sometimes a few at once, while the section is on screen and motion is welcome.
  useEffect(() => {
    const grid = gridRef.current;
    if (
      !grid ||
      traces.length === 0 ||
      !document.documentElement.classList.contains("tt-motion") ||
      !("IntersectionObserver" in window)
    )
      return;
    let timer = 0;
    let id = 0;
    const send = () => {
      const burst = Math.random() < 0.2 ? 2 + Math.floor(Math.random() * 2) : 1;
      const fresh = Array.from({ length: burst }, () => ({
        id: id++,
        trace: Math.floor(Math.random() * traces.length),
        duration: 1.6 + Math.random() * 2.2,
      }));
      setPulses((now) => [...now.slice(-24), ...fresh]);
      timer = window.setTimeout(send, 250 + Math.random() * 850);
    };
    const observer = new IntersectionObserver(([entry]) => {
      window.clearTimeout(timer);
      if (entry?.isIntersecting) send();
    });
    observer.observe(grid);
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, [traces]);

  useEffect(() => {
    const grid = gridRef.current;
    const core = coreRef.current;
    if (!grid || !core) return;
    const measure = () => {
      const box = grid.getBoundingClientRect();
      setSize({ width: box.width, height: box.height });
      if (!matchMedia("(min-width: 1024px)").matches) {
        setTraces([]);
        return;
      }
      const c = core.getBoundingClientRect();
      const coreLeft = c.left - box.left;
      const coreRight = c.right - box.left;
      const coreTop = c.top - box.top;
      const coreBottom = c.bottom - box.top;
      const next: Trace[] = [];
      const cards = [...grid.querySelectorAll<HTMLElement>("[data-pledge]")];
      for (const card of cards) {
        const side = card.dataset.pledge === "left" ? "left" : "right";
        const index = Number(card.dataset.index ?? 0);
        const r = card.getBoundingClientRect();
        const y = r.top - box.top + r.height / 2;
        const pinY = coreTop + (c.height * (index + 1.5)) / 4.5;
        if (side === "left") {
          const from = r.right - box.left;
          const turn = from + (coreLeft - from) * (0.3 + index * 0.18);
          next.push({
            d: rounded([
              { x: from, y },
              { x: turn, y },
              { x: turn, y: pinY },
              { x: coreLeft, y: pinY },
            ]),
            pin: { x: coreLeft, y: pinY, side },
          });
        } else {
          const from = r.left - box.left;
          const turn = from - (from - coreRight) * (0.3 + index * 0.18);
          next.push({
            d: rounded([
              { x: from, y },
              { x: turn, y },
              { x: turn, y: pinY },
              { x: coreRight, y: pinY },
            ]),
            pin: { x: coreRight, y: pinY, side },
          });
        }
      }
      // A few short traces into the core's top and bottom, from dots, for the circuit.
      const middle = (coreLeft + coreRight) / 2;
      for (const [k, dx] of [-90, 0, 70].entries()) {
        const x = middle + dx;
        const start = { x: x + (k === 0 ? -60 : k === 2 ? 50 : 0), y: Math.max(8, coreTop - 90) };
        next.push({
          d: rounded([start, { x, y: start.y }, { x, y: coreTop }]),
          dot: start,
          pin: { x, y: coreTop, side: "top" },
        });
      }
      for (const [k, dx] of [-50, 60].entries()) {
        const x = middle + dx;
        const start = { x: x + (k === 0 ? -40 : 40), y: Math.min(box.height - 8, coreBottom + 80) };
        next.push({
          d: rounded([start, { x, y: start.y }, { x, y: coreBottom }]),
          dot: start,
          pin: { x, y: coreBottom, side: "bottom" },
        });
      }
      setTraces(next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(grid);
    return () => observer.disconnect();
  }, []);

  const left = pledges.slice(0, 3);
  const right = pledges.slice(3, 6);
  const card = (pledge: Pledge, side: "left" | "right", index: number) => {
    return (
      <li
        key={pledge.title}
        data-pledge={side}
        data-index={index}
        data-reveal="blur"
        style={{ "--tt-delay": 80 + index * 90 } as CSSProperties}
        className="tt-pledge"
      >
        <span className="flex text-[var(--tt-accent-text)] [&_svg]:size-5">{pledge.icon}</span>
        <p className="mt-3 font-medium">{pledge.title}</p>
        <p className="mt-1.5 text-sm text-fd-muted-foreground">{pledge.body}</p>
      </li>
    );
  };

  return (
    <div ref={gridRef} className="tt-core-grid">
      {traces.length > 0 ? (
        <svg
          aria-hidden
          className="tt-traces"
          width={size.width}
          height={size.height}
          viewBox={`0 0 ${size.width} ${size.height}`}
        >
          {traces.map((trace, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: traces are measured anew as a set
            <g key={index}>
              <path d={trace.d} className="tt-trace" />
              {trace.dot ? (
                <circle cx={trace.dot.x} cy={trace.dot.y} r={3} className="tt-trace-dot" />
              ) : null}
              <rect
                x={
                  trace.pin.side === "left"
                    ? trace.pin.x - 7
                    : trace.pin.side === "right"
                      ? trace.pin.x
                      : trace.pin.x - 3
                }
                y={
                  trace.pin.side === "top"
                    ? trace.pin.y - 7
                    : trace.pin.side === "bottom"
                      ? trace.pin.y
                      : trace.pin.y - 3
                }
                width={trace.pin.side === "left" || trace.pin.side === "right" ? 7 : 6}
                height={trace.pin.side === "left" || trace.pin.side === "right" ? 6 : 7}
                rx={1.5}
                className="tt-trace-pin"
              />
            </g>
          ))}
          {pulses.map((pulse) => {
            const trace = traces[pulse.trace];
            return trace ? (
              <path
                key={pulse.id}
                d={trace.d}
                pathLength={1}
                className="tt-trace-pulse"
                style={{ "--duration": `${pulse.duration}s` } as CSSProperties}
                onAnimationEnd={() => setPulses((now) => now.filter((p) => p.id !== pulse.id))}
              />
            ) : null;
          })}
        </svg>
      ) : null}
      <ul className="tt-pledges tt-pledges-left">{left.map((p, i) => card(p, "left", i))}</ul>
      <div ref={coreRef} className="tt-core" data-reveal="scale">
        {children}
      </div>
      <ul className="tt-pledges tt-pledges-right">{right.map((p, i) => card(p, "right", i))}</ul>
    </div>
  );
}
