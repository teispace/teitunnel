import type { CSSProperties } from "react";

/** A terminal session, typed out line by line when it scrolls into view (see global.css). */
export function Terminal({
  title,
  lines,
}: {
  title: string;
  /** `$ ` starts a command; other lines are output. */
  lines: string[];
}) {
  return (
    <div
      data-reveal
      className="min-w-0 overflow-hidden rounded-xl border border-fd-border bg-[#0d0f12] text-[#e6e6e6] shadow-xl shadow-black/10 dark:shadow-black/40"
    >
      <div className="flex items-center gap-2 border-b border-white/10 px-4 py-2.5">
        <span className="size-2.5 rounded-full bg-white/15" />
        <span className="size-2.5 rounded-full bg-white/15" />
        <span className="size-2.5 rounded-full bg-white/15" />
        <span className="ms-2 font-mono text-xs text-white/50">{title}</span>
      </div>
      <pre className="overflow-x-auto p-4 font-mono text-[13px] leading-6">
        <code>
          {lines.map((line, index) => {
            const command = line.startsWith("$ ");
            return (
              <span
                // biome-ignore lint/suspicious/noArrayIndexKey: a fixed transcript
                key={index}
                className="tt-line block whitespace-pre-wrap break-words"
                style={{ "--tt-line": 250 + index * 240 } as CSSProperties}
              >
                {command ? (
                  <>
                    <span className="select-none text-[oklch(0.78_0.16_150)]">$ </span>
                    {line.slice(2)}
                  </>
                ) : (
                  <span className="text-white/60">{line || " "}</span>
                )}
              </span>
            );
          })}
          <span
            className="tt-line inline-block"
            style={{ "--tt-line": 250 + lines.length * 240 } as CSSProperties}
          >
            <span className="tt-caret inline-block h-4 w-2 translate-y-0.5 bg-white/70" />
          </span>
        </code>
      </pre>
    </div>
  );
}
