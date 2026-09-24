import { ChartNoAxesColumn, RefreshCw, TriangleAlert } from "lucide-react";
import { useMemo, useState } from "react";
import { useUiStore } from "@/app/ui-store";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { PermissionFix, useAccounts, useActiveAccount } from "@/features/accounts";
import { useRoutesOverview } from "@/features/routes/queries";
import { t } from "@/lib/i18n";
import type { AnalyticsRange } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useManualRefetch } from "@/lib/use-manual-refetch";
import { AnalyticsTable } from "./components/analytics-table";
import {
  buildRows,
  rangeOptions,
  type SortColumn,
  type SortDirection,
  sortRows,
  unavailableNote,
} from "./model";
import { useAnalyticsSummary, useUptimeList } from "./queries";

/** Every route of the account side by side: traffic, errors, P95 and uptime. */
export function AnalyticsPage() {
  const { data: accounts = [] } = useAccounts();
  const active = useActiveAccount();
  const setActive = useUiStore((state) => state.setActiveAccountId);
  const [range, setRange] = useState<AnalyticsRange>("day");
  const [sort, setSort] = useState<{ column: SortColumn; direction: SortDirection }>({
    column: "requests",
    direction: "descending",
  });
  const overview = useRoutesOverview(active?.id ?? null);
  const routes = overview.data?.routes ?? [];
  const hostnames = useMemo(
    () => [...new Set(routes.filter((r) => r.client === null).map((r) => r.hostname))].sort(),
    [routes],
  );
  const summary = useAnalyticsSummary(active?.id ?? null, hostnames, range);
  const uptimes = useUptimeList();
  const reload = useManualRefetch(() =>
    Promise.all([overview.refetch(), summary.refetch(), uptimes.refetch()]),
  );
  const rows = useMemo(
    () =>
      sortRows(
        buildRows(routes, summary.data, uptimes.data ?? [], range),
        sort.column,
        sort.direction,
      ),
    [routes, summary.data, uptimes.data, range, sort],
  );
  const onSort = (column: SortColumn) =>
    setSort((current) =>
      current.column === column
        ? {
            column,
            direction: current.direction === "ascending" ? "descending" : "ascending",
          }
        : { column, direction: column === "route" ? "ascending" : "descending" },
    );
  const edgeError = summary.error ? toIpcError(summary.error) : null;
  const note = summary.data ? unavailableNote(summary.data.unavailable) : null;

  const toolbar = (
    <TitlebarToolbar title={t("analytics.title")}>
      {accounts.length > 1 && active ? (
        <Select
          label={t("common.account")}
          options={accounts.map((a) => ({ value: a.id, label: a.name }))}
          value={active.id}
          onValueChange={setActive}
        />
      ) : null}
      <SegmentedControl
        label={t("analytics.rangeLabel")}
        segments={rangeOptions()}
        value={range}
        onValueChange={setRange}
      />
      <IconButton
        icon={RefreshCw}
        label={t("analytics.refresh")}
        onClick={reload.refresh}
        pending={reload.refreshing}
      />
    </TitlebarToolbar>
  );

  const body = (() => {
    if (overview.error) {
      const error = toIpcError(overview.error);
      return (
        <ErrorState
          title={t("routes.loadFailed")}
          message={error.message}
          hint={error.hint}
          action={
            <Button pending={overview.isFetching} onClick={() => void overview.refetch()}>
              {t("common.tryAgain")}
            </Button>
          }
        />
      );
    }
    if (!active || overview.isPending) {
      return (
        <div
          role="status"
          aria-busy
          aria-label={t("analytics.title")}
          className="flex flex-col gap-2 p-5"
        >
          <Skeleton className="h-11" />
          <Skeleton className="h-11" />
          <Skeleton className="h-11" />
        </div>
      );
    }
    if (rows.length === 0) {
      return (
        <EmptyState
          icon={ChartNoAxesColumn}
          title={t("analytics.empty.title")}
          description={t("analytics.empty.description")}
        />
      );
    }
    return (
      <div className="mx-auto flex w-full max-w-[860px] flex-col gap-4 overflow-y-auto px-5 pt-2 pb-8">
        {edgeError?.code === "permissionDenied" ? (
          <PermissionFix
            accountId={active.id}
            needs={[{ kind: "analytics" }]}
            refused
            onReady={() => void summary.refetch()}
          />
        ) : edgeError ? (
          <div
            role="alert"
            className="flex items-center gap-3 rounded-card bg-surface-inset px-3 py-2.5"
          >
            <TriangleAlert
              aria-hidden
              className="size-4 shrink-0 text-warning"
              strokeWidth={1.75}
            />
            <span className="min-w-0 flex-1 text-body">
              {t("analytics.loadFailed")}{" "}
              <span className="text-secondary">{edgeError.message}</span>
            </span>
            <Button size="sm" pending={summary.isFetching} onClick={() => void summary.refetch()}>
              {t("common.tryAgain")}
            </Button>
          </div>
        ) : null}
        <AnalyticsTable
          rows={rows}
          range={range}
          sort={sort}
          onSort={onSort}
          loading={summary.isPending && !edgeError}
        />
        <p className="px-2.5 text-callout text-secondary">{t("analytics.edgeNote")}</p>
        {note ? <p className="px-2.5 text-callout text-secondary">{note}</p> : null}
      </div>
    );
  })();

  return (
    <>
      {toolbar}
      <div className="flex min-h-0 flex-1 flex-col">{body}</div>
    </>
  );
}
