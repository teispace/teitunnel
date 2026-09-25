import { Link } from "@tanstack/react-router";
import { ChevronDown, ChevronUp } from "lucide-react";
import { Sparkline } from "@/components/patterns/sparkline";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { cn } from "@/lib/cn";
import { type MessageKey, t } from "@/lib/i18n";
import type { AnalyticsRange } from "@/lib/ipc/bindings";
import {
  formatCount,
  formatMs,
  formatPercent,
  type Row,
  rangeNoun,
  type SortColumn,
  type SortDirection,
} from "../model";

const COLUMNS: readonly { id: SortColumn; label: MessageKey; width: string }[] = [
  { id: "route", label: "analytics.column.route", width: "" },
  { id: "requests", label: "analytics.column.requests", width: "w-36" },
  { id: "errors", label: "analytics.column.errors", width: "w-24" },
  { id: "p95", label: "analytics.column.p95", width: "w-18" },
  { id: "uptime", label: "analytics.column.uptime", width: "w-18" },
];

function statusOf(row: Row): { dot: Status; label: string } {
  if (!row.local || row.up === null) return { dot: "idle", label: t("uptime.unknown") };
  return row.up
    ? { dot: "healthy", label: t("uptime.up") }
    : { dot: "error", label: t("uptime.down") };
}

interface AnalyticsTableProps {
  rows: readonly Row[];
  range: AnalyticsRange;
  sort: { column: SortColumn; direction: SortDirection };
  onSort: (column: SortColumn) => void;
  /** The edge numbers are still loading (their cells stay empty). */
  loading: boolean;
}

/** Every route side by side: traffic, 5xx share, P95 and uptime; headers sort. */
export function AnalyticsTable({ rows, range, sort, onSort, loading }: AnalyticsTableProps) {
  const period = rangeNoun(range);
  const Icon = sort.direction === "ascending" ? ChevronUp : ChevronDown;
  return (
    <div className="rounded-card bg-surface-inset px-2.5">
      <table aria-label={t("analytics.list")} className="w-full table-fixed border-collapse">
        <thead>
          <tr>
            {COLUMNS.map((column) => {
              const active = sort.column === column.id;
              return (
                <th
                  key={column.id}
                  scope="col"
                  aria-sort={active ? sort.direction : "none"}
                  className={cn(
                    "h-8 font-normal",
                    column.width,
                    column.id === "route" ? "text-left" : "text-right",
                  )}
                >
                  <button
                    type="button"
                    aria-label={t("analytics.sortBy", { column: t(column.label) })}
                    onClick={() => onSort(column.id)}
                    className={cn(
                      "inline-flex items-center gap-0.5 rounded-control text-footnote outline-offset-1",
                      active ? "text-primary" : "text-secondary",
                    )}
                  >
                    {t(column.label)}
                    {active ? <Icon aria-hidden className="size-3" strokeWidth={2} /> : null}
                  </button>
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const status = statusOf(row);
            return (
              <tr key={row.key} className="h-11 border-inset border-t-hairline">
                <td className="pr-3">
                  <span className="flex min-w-0 items-center gap-2.5">
                    <StatusDot status={status.dot} label={status.label} />
                    <Link
                      to="/routes"
                      className="selectable min-w-0 truncate text-body outline-offset-0"
                    >
                      {row.key}
                    </Link>
                    {row.local ? null : (
                      <span className="shrink-0 text-footnote text-secondary">
                        {t("analytics.notHere")}
                      </span>
                    )}
                  </span>
                </td>
                <td>
                  <span className="flex items-center gap-2">
                    <span className="min-w-0 flex-1">
                      {row.spark.length > 1 ? (
                        <Sparkline
                          values={row.spark}
                          label={t("analytics.sparkline", { route: row.key, period })}
                          height={20}
                        />
                      ) : null}
                    </span>
                    <span className="w-12 text-right text-callout tabular">
                      {loading && row.requests === null ? "" : formatCount(row.requests ?? 0)}
                    </span>
                  </span>
                </td>
                <td
                  className={cn(
                    "text-right text-callout tabular",
                    (row.errorRate ?? 0) >= 0.05 ? "text-error" : "text-secondary",
                  )}
                >
                  {formatPercent(row.errorRate)}
                </td>
                <td className="text-right text-callout text-secondary tabular">
                  {formatMs(row.p95)}
                </td>
                <td className="text-right text-callout tabular">
                  {row.local ? formatPercent(row.uptime) : "–"}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
