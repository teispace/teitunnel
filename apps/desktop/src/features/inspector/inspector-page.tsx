import { useNavigate } from "@tanstack/react-router";
import {
  FileOutput,
  GitCompareArrows,
  OctagonPause,
  Pause,
  Play,
  ScanSearch,
  SlidersHorizontal,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { useIssues } from "@/features/doctor/queries";
import { t } from "@/lib/i18n";
import type { ExchangeDetail as Detail, ExchangeRow, KnownTap } from "@/lib/ipc/bindings";
import { useDebounced } from "@/lib/use-debounced";
import { CompareSheet } from "./components/compare-sheet";
import { ExchangeDetail } from "./components/exchange-detail";
import { ExchangeList } from "./components/exchange-list";
import { ExportSheet } from "./components/export-sheet";
import { FilterBar } from "./components/filter-bar";
import { HeldRequest } from "./components/held-request";
import { InspectRouteSheet } from "./components/inspect-route-sheet";
import { ReplaySheet } from "./components/replay-sheet";
import { TapSettingsSheet } from "./components/tap-settings-sheet";
import { useLiveExchanges, useRows } from "./live";
import { type Filters, formatMs, isFiltered, matches, noFilters } from "./model";
import {
  type InspectRouteTarget,
  useClearExchanges,
  useInspectedRoutes,
  useKnownTaps,
  useResumeAll,
  useTapMetrics,
  useTaps,
} from "./queries";

const ALL = "all";

interface InspectorPageProps {
  /** Show this tap's requests first (a share's id, or a route's tap). */
  tap?: string | undefined;
  /** Show this hostname's requests first (a route or a share on your domain). */
  host?: string | undefined;
  /** Open with this request selected. */
  exchange?: string | undefined;
}

/** Order for the picker: running taps first, then by name. */
const byRunning = (a: KnownTap, b: KnownTap) =>
  Number(b.running) - Number(a.running) || a.name.localeCompare(b.name);

/**
 * The Inspector: every request to inspected shares and routes on this computer, live.
 * All traffic or one share or route; filters and a search that looks into bodies; a
 * request's headers, bodies, timing and webhook signature; replay, compare and export;
 * and what the inspector does for each share or route.
 */
export function InspectorPage({ tap: wantedTap, host, exchange }: InspectorPageProps) {
  const navigate = useNavigate();
  const known = useKnownTaps();
  const taps = useTaps();
  const inspectedRoutes = useInspectedRoutes();
  const { issues } = useIssues();
  const [chosen, setChosen] = useState<string | null>(null);
  const hostTap = host
    ? taps.data?.find((tap) => tap.scope.kind === "route" && tap.scope.hostname === host)?.id
    : undefined;
  const tapId = chosen ?? wantedTap ?? hostTap ?? ALL;
  const current = tapId === ALL ? null : tapId;

  const [search, setSearch] = useState("");
  const [filters, setFilters] = useState<Filters>(noFilters);
  const settledSearch = useDebounced(search, 250);
  const live = useLiveExchanges(current, settledSearch);
  const rows = useRows(live.store);
  const visible = useMemo(() => rows.filter((row) => matches(row, filters)), [rows, filters]);

  const [selectedId, setSelectedId] = useState<string | null>(exchange ?? null);
  const [marked, setMarked] = useState<ReadonlySet<string>>(new Set());
  const [replaying, setReplaying] = useState<Detail | null>(null);
  const [exporting, setExporting] = useState<readonly string[]>([]);
  const [comparing, setComparing] = useState<readonly [string, string] | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [restoring, setRestoring] = useState<InspectRouteTarget | null>(null);
  const clear = useClearExchanges();
  const tapView = taps.data?.find((tap) => tap.id === current) ?? null;
  const metrics = useTapMetrics(tapView ? tapView.id : null);

  const selected = visible.find((row) => row.id === selectedId) ?? null;
  const held = useMemo(() => rows.filter((row) => row.paused), [rows]);
  const resumeAll = useResumeAll();
  // Land on a request as soon as it's held, unless something else is being looked at.
  const firstHeld = held.at(-1)?.id ?? null;
  useEffect(() => {
    if (firstHeld && selectedId === null) setSelectedId(firstHeld);
  }, [firstHeld, selectedId]);
  const names = useMemo(
    () => new Map((known.data ?? []).map((tap) => [tap.id, tap.name])),
    [known.data],
  );
  const picked = [...marked];
  const pair: readonly [string, string] | null =
    picked.length === 2
      ? [picked[0] as string, picked[1] as string]
      : picked.length === 1 && selectedId && picked[0] !== selectedId
        ? [picked[0] as string, selectedId]
        : null;
  const exportIds = picked.length > 0 ? picked : selectedId ? [selectedId] : [];
  const orphans = (inspectedRoutes.data ?? []).filter((route) =>
    issues.some((i) => i.check === "inspect.orphan" && i.subject === route.hostname),
  );

  const select = (id: string, { toggle }: { toggle: boolean }) => {
    if (!toggle) {
      setSelectedId(id);
      setMarked(new Set());
      return;
    }
    const next = new Set(marked);
    if (selectedId && next.size === 0 && selectedId !== id) next.add(selectedId);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setMarked(next);
    setSelectedId(id);
  };

  const onReplayed = (replays: ExchangeRow[]) => {
    const newest = replays[replays.length - 1];
    if (newest) {
      setSelectedId(newest.id);
      setMarked(new Set());
    }
  };

  const tapOptions = [
    { value: ALL, label: t("inspector.allTraffic") },
    ...[...(known.data ?? [])].sort(byRunning).map((tap) => ({
      value: tap.id,
      label: tap.running ? tap.name : t("inspector.stopped", { name: tap.name }),
    })),
    ...(current && !(known.data ?? []).some((tap) => tap.id === current)
      ? [{ value: current, label: names.get(current) ?? current }]
      : []),
  ];

  const toolbar = (
    <TitlebarToolbar title={t("inspector.title")}>
      <Select
        label={t("inspector.tapLabel")}
        options={tapOptions}
        value={tapId}
        onValueChange={(value) => {
          setChosen(value);
          setSelectedId(null);
          setMarked(new Set());
        }}
        className="max-w-56"
      />
      <IconButton
        icon={live.paused ? Play : Pause}
        label={live.paused ? t("inspector.resume") : t("inspector.pause")}
        aria-pressed={live.paused}
        onClick={() => live.setPaused(!live.paused)}
      />
      <ConfirmDialog
        trigger={
          <IconButton icon={Trash2} label={t("inspector.clear")} disabled={rows.length === 0} />
        }
        title={current ? t("inspector.clearTapTitle") : t("inspector.clearAllTitle")}
        description={t("inspector.clearDetail")}
        confirmLabel={t("inspector.clearConfirm")}
        variant="destructive"
        onConfirm={async () => {
          await clear.mutateAsync(current);
          live.store.reset([]);
          setSelectedId(null);
          setMarked(new Set());
        }}
      />
      <IconButton
        icon={GitCompareArrows}
        label={t("inspector.compareButton")}
        disabled={pair === null}
        onClick={() => pair && setComparing(pair)}
      />
      <IconButton
        icon={FileOutput}
        label={t("inspector.exportButton")}
        disabled={exportIds.length === 0}
        onClick={() => setExporting(exportIds)}
      />
      <IconButton
        icon={SlidersHorizontal}
        label={t("inspector.tap.open")}
        disabled={!tapView}
        onClick={() => setSettingsOpen(true)}
      />
    </TitlebarToolbar>
  );

  const loaded = known.isSuccess && live.status === "ready";
  const nothingYet =
    loaded && rows.length === 0 && !settledSearch.trim() && (known.data ?? []).length === 0;

  const list = (
    <>
      {orphans.map((route) => (
        <div
          key={`${route.accountId}/${route.hostname}${route.path ?? ""}`}
          role="alert"
          className="mx-3 mt-2 flex items-center gap-2 rounded-row bg-warning/10 px-3 py-2 text-callout"
        >
          <span className="min-w-0 flex-1">
            {t("inspector.orphan", { hostname: route.hostname })}
          </span>
          <Button
            size="sm"
            onClick={() =>
              setRestoring({
                accountId: route.accountId,
                hostname: route.hostname,
                path: route.path,
                on: false,
              })
            }
          >
            {t("inspector.route.restore")}
          </Button>
        </div>
      ))}
      {held.length > 0 ? (
        <div
          role="status"
          className="mx-3 mt-2 flex items-center gap-2 rounded-row bg-accent/10 px-3 py-2 text-callout"
        >
          <OctagonPause aria-hidden className="size-4 shrink-0 text-accent" strokeWidth={2} />
          <span className="min-w-0 flex-1">
            {t("inspector.held.banner", { count: held.length })}
          </span>
          <Button size="sm" pending={resumeAll.isPending} onClick={() => resumeAll.mutate(current)}>
            {t("inspector.held.continueAll")}
          </Button>
        </div>
      ) : null}
      {live.status === "loading" && rows.length === 0 ? (
        <div className="flex flex-col gap-1.5 px-3 pt-8" aria-busy>
          {Array.from({ length: 6 }, (_, row) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: placeholder rows are positional
            <Skeleton key={row} className="h-4 w-full" />
          ))}
        </div>
      ) : (
        <ExchangeList
          rows={visible}
          selectedId={selectedId}
          marked={marked}
          onSelect={select}
          empty={
            rows.length === 0
              ? settledSearch.trim()
                ? t("inspector.noResults")
                : t("inspector.waiting")
              : t("inspector.noMatch")
          }
          footer={
            live.hasOlder ? (
              <div className="flex justify-center py-2">
                <Button size="sm" pending={live.loadingOlder} onClick={live.loadOlder}>
                  {t("inspector.loadOlder")}
                </Button>
              </div>
            ) : null
          }
        />
      )}
      <footer className="flex h-7 shrink-0 items-center gap-2 border-separator border-t-hairline px-3 text-callout text-secondary tabular">
        <span aria-live="polite">
          {isFiltered(filters) || settledSearch.trim()
            ? t("inspector.shownOf", { shown: visible.length, count: rows.length })
            : t("inspector.count", { count: rows.length })}
        </span>
        {metrics.data && metrics.data.latency.p95Ms !== null ? (
          <span>· {t("inspector.p95", { duration: formatMs(metrics.data.latency.p95Ms) })}</span>
        ) : null}
        {live.paused ? (
          <span className="ml-auto text-warning">
            {t("inspector.pausedWaiting", { count: live.waiting })}
          </span>
        ) : null}
      </footer>
    </>
  );

  return (
    <>
      {toolbar}
      {live.status === "error" && rows.length === 0 ? (
        <ErrorState
          title={t("inspector.error")}
          message={live.error ?? ""}
          action={<Button onClick={live.reload}>{t("common.tryAgain")}</Button>}
        />
      ) : nothingYet ? (
        <EmptyState
          icon={ScanSearch}
          title={t("inspector.empty.title")}
          description={t("inspector.empty.description")}
          action={
            <Button variant="primary" onClick={() => void navigate({ to: "/quick-share" })}>
              {t("inspector.empty.action")}
            </Button>
          }
        />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          <FilterBar
            search={search}
            onSearch={setSearch}
            filters={filters}
            onFilters={setFilters}
          />
          <SplitView
            id="inspector"
            list={list}
            defaultListWidth={520}
            minListWidth={340}
            maxListWidth={900}
          >
            {selected?.paused ? (
              <HeldRequest key={selected.id} row={selected} />
            ) : selected ? (
              <ExchangeDetail
                key={selected.id}
                row={selected}
                tapName={names.get(selected.tap) ?? null}
                onReplayed={onReplayed}
                onEdit={setReplaying}
                onExport={() => setExporting(exportIds)}
              />
            ) : (
              <p className="m-auto max-w-60 px-4 text-center text-callout text-secondary">
                {marked.size > 1
                  ? t("inspector.detail.several", { count: marked.size })
                  : t("inspector.detail.select")}
              </p>
            )}
          </SplitView>
        </div>
      )}
      <ReplaySheet detail={replaying} onClose={() => setReplaying(null)} onReplayed={onReplayed} />
      <ExportSheet ids={exporting} onClose={() => setExporting([])} />
      <CompareSheet pair={comparing} onClose={() => setComparing(null)} />
      <TapSettingsSheet tap={tapView} open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <InspectRouteSheet target={restoring} restore onClose={() => setRestoring(null)} />
    </>
  );
}
