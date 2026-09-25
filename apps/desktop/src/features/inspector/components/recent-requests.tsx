import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { useLiveExchanges, useRows } from "../live";
import { formatClock, formatMs, statusClass, statusText } from "../model";

interface RecentRequestsProps {
  /** How many to show. */
  limit: number;
  /** Wraps the rows (a titled section); nothing is shown before the first request. */
  children: (rows: ReactNode) => ReactNode;
}

/**
 * The newest requests to every inspected share and route, live; each opens in the
 * Inspector. Reads only `limit` requests and keeps no more.
 */
export function RecentRequests({ limit, children }: RecentRequestsProps) {
  const live = useLiveExchanges(null, "", limit);
  const rows = useRows(live.store);
  if (rows.length === 0) return null;
  return children(
    rows.map((row) => {
      return (
        <li key={row.id}>
          <Link
            to="/inspector"
            search={{ exchange: row.id }}
            title={`${row.method} ${row.host}${row.path}`}
            className="flex min-h-9 items-center gap-3 rounded-row py-1.5 font-mono text-mono outline-offset-0"
          >
            <span className={cn("w-10 shrink-0 tabular", statusClass(row))}>{statusText(row)}</span>
            <span className="w-14 shrink-0 truncate font-medium">{row.method}</span>
            <span className="min-w-0 flex-1 truncate">
              <span className="text-secondary">{row.host}</span>
              {row.path}
            </span>
            <span className="w-16 shrink-0 text-right text-secondary tabular">
              {row.durationMs === null ? "" : formatMs(row.durationMs)}
            </span>
            <span className="w-16 shrink-0 text-right text-secondary tabular">
              {formatClock(row.startedAt)}
            </span>
          </Link>
        </li>
      );
    }),
  );
}
