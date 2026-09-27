import type { CSSProperties } from "react";

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
