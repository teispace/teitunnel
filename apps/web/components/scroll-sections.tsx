import type { ComponentType, CSSProperties } from "react";

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
