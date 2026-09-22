import { cn } from "@/lib/cn";
import type { LogLine } from "@/lib/ipc/bindings";

/** The newest cloudflared log lines, coloured by level (the full viewer arrives in M5). */
export function LogLines({ lines, empty }: { lines: readonly LogLine[]; empty: string }) {
  if (lines.length === 0) {
    return <p className="py-2 text-callout text-secondary">{empty}</p>;
  }
  return (
    <ol className="selectable max-h-56 overflow-y-auto rounded-control bg-surface-inset px-2 py-1.5 font-mono text-[11px] leading-4">
      {lines.map((line, index) => (
        <li
          // biome-ignore lint/suspicious/noArrayIndexKey: log lines are append-only snapshots
          key={index}
          className={cn(
            "break-words",
            (line.level === "error" || line.level === "fatal") && "text-error",
            line.level === "warn" && "text-warning",
          )}
        >
          {line.message}
          {line.error ? <span className="text-secondary"> {line.error}</span> : null}
        </li>
      ))}
    </ol>
  );
}
