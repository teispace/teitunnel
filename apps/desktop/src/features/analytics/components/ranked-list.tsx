import { cn } from "@/lib/cn";
import type { Ranked } from "@/lib/ipc/bindings";
import { formatCount } from "../model";

interface RankedListProps {
  title: string;
  rows: readonly Ranked[];
  /** Shown for an empty key (e.g. "not a verified bot"). */
  emptyKey?: string;
  /** Values that are hostnames or paths use the monospaced face. */
  mono?: boolean;
  limit?: number;
}

/** Top values of a breakdown, each with its share of the list as a thin bar behind it. */
export function RankedList({ title, rows, emptyKey, mono = false, limit = 5 }: RankedListProps) {
  if (rows.length === 0) return null;
  const shown = rows.slice(0, limit);
  const max = Math.max(1, ...shown.map((r) => r.requests));
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <h4 className="text-callout text-secondary">{title}</h4>
      <ul className="flex flex-col gap-0.5">
        {shown.map((row) => (
          <li key={row.key} className="relative flex min-h-5 items-center gap-2 px-1.5">
            <span
              aria-hidden
              className="absolute inset-y-0 left-0 rounded-[3px] bg-accent/12"
              style={{ width: `${(row.requests / max) * 100}%` }}
            />
            <span
              className={cn(
                "selectable relative min-w-0 flex-1 truncate",
                mono ? "font-mono text-mono" : "text-callout",
              )}
            >
              {row.key === "" ? (emptyKey ?? "–") : row.key}
            </span>
            <span className="relative text-callout text-secondary tabular">
              {formatCount(row.requests)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
