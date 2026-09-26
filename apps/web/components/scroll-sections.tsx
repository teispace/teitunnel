import type { ComponentType, CSSProperties, ReactNode } from "react";
import { CopyCommand } from "./copy-command";

/** `--i` and friends as inline style. */
const vars = (values: Record<string, number | string>) => values as CSSProperties;

/**
 * A statement that lights up word by word as it's scrolled: the section stays on screen
 * while the words go from faint to full. Emphasised words (wrapped in `*`) take the accent.
 */
export function Manifesto({ text, label }: { text: string; label: string }) {
  // Words between `*` markers are emphasised, e.g. "*No open ports.*".
  let inside = false;
  const words = text.split(" ").map((raw) => {
    const opens = raw.startsWith("*");
    const closes = raw.endsWith("*");
    const accent = inside || opens;
    if (opens) inside = true;
    if (closes) inside = false;
    return { word: raw.replaceAll("*", ""), accent };
  });
  return (
    <section aria-label={label} data-scene="pin" className="tt-manifesto relative">
      <div className="tt-manifesto-stage">
        <div className="tt-stars" aria-hidden>
          <span />
        </div>
        <p
          className="tt-manifesto-text mx-auto max-w-5xl px-6 text-[2rem] leading-[1.12] font-semibold tracking-tight text-balance sm:text-5xl md:text-6xl"
          style={vars({ "--n": words.length })}
        >
          {words.map(({ word, accent }, index) => (
            <span
              // biome-ignore lint/suspicious/noArrayIndexKey: a fixed sentence
              key={index}
              className={`tt-word${accent ? " tt-word-accent" : ""}`}
              style={vars({ "--i": index })}
            >
              {word}{" "}
            </span>
          ))}
        </p>
      </div>
    </section>
  );
}

export interface ToolChip {
  icon: ComponentType<{ className?: string }>;
  name: string;
  note: string;
}

function Chip({ icon: Icon, name, note, copy }: ToolChip & { copy: boolean }) {
  return (
    <span
      data-copy={copy ? "" : undefined}
      className="tt-chip flex shrink-0 items-center gap-3 rounded-2xl border border-fd-border bg-fd-background px-4 py-3"
    >
      <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-fd-card">
        <Icon className="size-4.5 text-fd-foreground" aria-hidden />
      </span>
      <span>
        <span className="block text-sm font-medium whitespace-nowrap">{name}</span>
        <span className="block text-xs whitespace-nowrap text-fd-muted-foreground">{note}</span>
      </span>
    </span>
  );
}

/**
 * The tools Teitunnel works with, drifting past in two rows going opposite ways; hovering
 * holds them. The list is read once (the moving copies are hidden from screen readers), and
 * without motion it's a plain wrapped list.
 */
export function ToolMarquee({ tools }: { tools: ToolChip[] }) {
  const half = Math.ceil(tools.length / 2);
  const rows = [tools.slice(0, half), tools.slice(half)];
  return (
    <div className="tt-marquee flex flex-col gap-3" data-reveal="blur">
      <ul className="sr-only">
        {tools.map((tool) => (
          <li key={tool.name}>
            {tool.name}: {tool.note}
          </li>
        ))}
      </ul>
      {rows.map((row, index) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: two fixed rows
          key={index}
          aria-hidden
          className="tt-marquee-row"
          data-direction={index === 0 ? "left" : "right"}
        >
          <div className="tt-marquee-track">
            {[...row, ...row, ...row].map((tool, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: repeated copies of a fixed list
              <Chip key={`${tool.name}-${i}`} {...tool} copy={i >= row.length} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

export interface Pledge {
  icon: ComponentType<{ className?: string }>;
  title: string;
  body: string;
}

/**
 * Security promises flying past a headline that stays in place (wide screens with motion):
 * each card rises through the view at its own speed. Elsewhere, a grid.
 */
export function PromiseField({ promises, children }: { promises: Pledge[]; children: ReactNode }) {
  // Where each card flies: its column (percent from the left) and speed.
  const lanes = [
    { x: 4, speed: 1.1, start: 0.02 },
    { x: 71, speed: 0.95, start: 0.1 },
    { x: 7, speed: 1.05, start: 0.24 },
    { x: 68, speed: 1.15, start: 0.33 },
    { x: 3, speed: 0.95, start: 0.45 },
    { x: 72, speed: 1.1, start: 0.54 },
  ];
  return (
    <section
      id="security"
      aria-labelledby="security-title"
      data-scene="pin"
      className="tt-field relative"
    >
      <div className="tt-field-stage">
        <div className="tt-field-center mx-auto max-w-3xl px-6 text-center">{children}</div>
        <ul className="tt-field-cards">
          {promises.map(({ icon: Icon, title, body }, index) => {
            const lane = lanes[index % lanes.length] ?? { x: 0, speed: 1, start: 0 };
            return (
              <li
                key={title}
                className="tt-field-card"
                style={vars({ "--x": `${lane.x}%`, "--speed": lane.speed, "--start": lane.start })}
              >
                <Icon className="size-5 text-[var(--tt-accent-text)]" aria-hidden />
                <p className="mt-3 font-medium">{title}</p>
                <p className="mt-1.5 text-sm text-fd-muted-foreground">{body}</p>
              </li>
            );
          })}
        </ul>
      </div>
    </section>
  );
}

/** Three steps joined by a line that draws itself as they're scrolled into view. */
export function StepLine({ steps }: { steps: { title: string; body: string }[] }) {
  return (
    <div data-scene="enter" data-end="0.55" className="tt-steps relative">
      <span className="tt-steps-line" aria-hidden />
      <ol className="grid gap-4 md:grid-cols-3">
        {steps.map((step, index) => (
          <li
            key={step.title}
            className="tt-step-card relative flex gap-4 rounded-2xl border border-fd-border bg-fd-background p-6"
            style={vars({ "--i": index })}
          >
            <span className="tt-step-num flex size-8 shrink-0 items-center justify-center rounded-full border border-fd-border bg-fd-background font-mono text-sm">
              {index + 1}
            </span>
            <div>
              <h3 className="font-medium">{step.title}</h3>
              <p className="mt-1 text-sm text-fd-muted-foreground">{step.body}</p>
            </div>
          </li>
        ))}
      </ol>
    </div>
  );
}

/**
 * The install command, huge, lit where the pointer is (and slowly swept by light without a
 * pointer), with the copyable command under it.
 */
export function BigCommand({ command, children }: { command: string; children?: ReactNode }) {
  return (
    <div data-scene="enter" data-end="0.2" className="tt-finale">
      <div className="tt-finale-card" data-spotlight>
        <p aria-hidden className="tt-finale-command font-mono">
          <span className="text-fd-muted-foreground/60">$ </span>
          {command}
        </p>
        <div className="relative mt-8 flex justify-center">
          <CopyCommand command={command} label="Copy the Homebrew command" />
        </div>
        {children}
      </div>
    </div>
  );
}
