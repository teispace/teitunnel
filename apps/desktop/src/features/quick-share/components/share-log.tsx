import { cn } from "@/lib/cn";
import { useShareLogs } from "../queries";

/** The newest cloudflared log lines (the full log viewer arrives in M5). */
export function ShareLog({ id }: { id: string }) {
  const { data: lines = [] } = useShareLogs(id, true);
  if (lines.length === 0) {
    return <p className="py-2 text-callout text-secondary">No log output yet.</p>;
  }
  return (
    <ol className="selectable max-h-56 overflow-y-auto rounded-control bg-surface-content px-2 py-1.5 font-mono text-[11px] leading-4">
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
