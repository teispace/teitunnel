import {
  Activity,
  Camera,
  Copy,
  Globe,
  Inbox,
  Key,
  LockKeyhole,
  type LucideIcon,
  Network,
  RefreshCw,
  Route as RouteIcon,
  Shield,
  Split,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { useUiStore } from "@/app/ui-store";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { useAccounts, useActiveAccount } from "@/features/accounts";
import { PlanSteps, useActivity, useRoutesOverview, useVerify } from "@/features/routes";
import { cn } from "@/lib/cn";
import { type MessageKey, t, translate } from "@/lib/i18n";
import type { ActivityEntry, ActivityRecord, Delta } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useManualRefetch } from "@/lib/use-manual-refetch";
import { commandScript, leftoversOf, matches, type Show, showOptions, summaryOf } from "./model";

const outcomes: Record<string, { dot: Status; label: MessageKey }> = {
  applied: { dot: "healthy", label: "activity.outcome.applied" },
  rolledBack: { dot: "warning", label: "activity.outcome.rolledBack" },
  partiallyApplied: { dot: "error", label: "activity.outcome.partiallyApplied" },
  alert: { dot: "error", label: "activity.outcome.alert" },
  resolved: { dot: "healthy", label: "activity.outcome.resolved" },
};

/** The Domain menu's "any" item (Radix Select items can't have an empty value). */
const ANY_DOMAIN = "*";

function outcomeOf(entry: ActivityEntry) {
  const outcome = outcomes[entry.outcome];
  return outcome
    ? { dot: outcome.dot, label: t(outcome.label) }
    : { dot: "idle" as const, label: entry.outcome };
}

/** "Applied · By Claude Code" for a change an AI agent made; the outcome otherwise. */
function outcomeLine(entry: ActivityEntry) {
  const actor = entry.record?.actor;
  const outcome = outcomeOf(entry).label;
  return actor ? `${outcome} · ${t("activity.byAgent", { client: actor.client })}` : outcome;
}

function day(at: number | null) {
  if (at === null) return t("time.earlier");
  const date = new Date(at);
  const today = new Date();
  const yesterday = new Date(today.getTime() - 86_400_000);
  if (date.toDateString() === today.toDateString()) return t("time.today");
  if (date.toDateString() === yesterday.toDateString()) return t("time.yesterday");
  return date.toLocaleDateString(undefined, { dateStyle: "medium" });
}

function time(at: number | null) {
  return at === null
    ? ""
    : new Date(at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** Every change Teitunnel made to Cloudflare, newest first, with its steps. */
export function ActivityPage() {
  const { data: accounts = [], isSuccess } = useAccounts();
  const active = useActiveAccount();
  const setActive = useUiStore((state) => state.setActiveAccountId);
  const activity = useActivity(active?.id ?? null);
  const reload = useManualRefetch(activity.refetch);
  const overview = useRoutesOverview(active?.id ?? null).data;
  const zones = overview?.zones ?? [];
  const all = activity.data ?? [];
  const [show, setShow] = useState<Show>("all");
  const [zone, setZone] = useState<string>(ANY_DOMAIN);
  const [query, setQuery] = useState("");
  const filter = {
    show,
    zone: zone === ANY_DOMAIN ? null : zone,
    query: query.trim().toLowerCase(),
  };
  const entries = all.filter((e) => matches(e, filter));
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = entries.find((e) => String(e.id) === selectedId) ?? entries[0] ?? null;

  const toolbar = (
    <TitlebarToolbar title={t("activity.title")}>
      {accounts.length > 1 && active ? (
        <Select
          label={t("common.account")}
          options={accounts.map((a) => ({ value: a.id, label: a.name }))}
          value={active.id}
          onValueChange={setActive}
        />
      ) : null}
      {active ? (
        <IconButton
          icon={RefreshCw}
          label={t("activity.refresh")}
          onClick={reload.refresh}
          pending={reload.refreshing}
        />
      ) : null}
    </TitlebarToolbar>
  );

  const body = activity.error ? (
    <ErrorState
      title={t("activity.loadFailed")}
      message={toIpcError(activity.error).message}
      action={<Button onClick={() => void activity.refetch()}>{t("common.tryAgain")}</Button>}
    />
  ) : activity.isPending && active ? (
    <div className="flex flex-col gap-2 p-3">
      <Skeleton className="h-11" />
    </div>
  ) : all.length === 0 || (isSuccess && !active) ? (
    <EmptyState
      icon={Activity}
      title={t("activity.empty.title")}
      description={t("activity.empty.description")}
    />
  ) : (
    <SplitView
      id="activity"
      list={
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="flex flex-col gap-1.5 px-2.5 pt-1 pb-1.5">
            <Input
              type="search"
              aria-label={t("activity.filter")}
              placeholder={t("activity.filterPlaceholder")}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              className="rounded-full"
            />
            <div className="flex gap-1.5">
              <Select
                label={t("activity.showLabel")}
                options={showOptions()}
                value={show}
                onValueChange={setShow}
                className="min-w-0 flex-1"
              />
              {zones.length > 1 ? (
                <Select
                  label={t("activity.domain")}
                  options={[
                    { value: ANY_DOMAIN, label: t("activity.allDomains") },
                    ...zones.map((z) => ({ value: z.name, label: z.name })),
                  ]}
                  value={zone}
                  onValueChange={setZone}
                  className="min-w-0 flex-1"
                />
              ) : null}
            </div>
          </div>
          <ListPane
            label={t("activity.list")}
            items={entries}
            getId={(e) => String(e.id)}
            groupOf={(e) => day(e.at)}
            selectedId={selected ? String(selected.id) : null}
            onSelect={setSelectedId}
            renderRow={(entry) => (
              <ListRow
                title={summaryOf(entry)}
                subtitle={`${time(entry.at)} · ${outcomeLine(entry)}`}
                leading={<StatusDot status={outcomeOf(entry).dot} label={outcomeOf(entry).label} />}
              />
            )}
            empty={
              <EmptyState
                title={t("activity.noMatches.title")}
                description={t("activity.noMatches.description")}
              />
            }
          />
        </div>
      }
    >
      {selected && active ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <Inspector
            title={summaryOf(selected)}
            subtitle={`${t("time.dayAt", { day: day(selected.at), time: time(selected.at) })} · ${outcomeLine(selected)}`}
          >
            {selected.record ? (
              <RecordDetails
                accountId={active.id}
                outcome={selected.outcome}
                record={selected.record}
                leftovers={leftoversOf(selected)}
                routed={overview?.routes.map((route) => route.hostname) ?? []}
              />
            ) : (
              <PlainSteps lines={selected.detail} />
            )}
          </Inspector>
        </div>
      ) : null}
    </SplitView>
  );

  return (
    <>
      {toolbar}
      {body}
    </>
  );
}

/** The structured record: what changed, the steps as they ended, and checks. */
function RecordDetails({
  accountId,
  outcome,
  record,
  leftovers,
  routed,
}: {
  accountId: string;
  outcome: string;
  record: ActivityRecord;
  /** What a failed undo left in place (failures show on their step). */
  leftovers: readonly string[];
  /** Hostnames routed now: only those can be checked again. */
  routed: readonly string[];
}) {
  const steps = record.steps.map((s) => s.step);
  const states = Object.fromEntries(record.steps.map((s, index) => [index, s.state]));
  const script = commandScript(steps);
  const checkable = record.hostnames.filter((h) => routed.includes(h));
  return (
    <>
      {record.changes.length > 0 ? (
        // A failed change didn't happen (or only partly): don't present it as done.
        <InspectorSection
          title={outcome === "applied" ? t("activity.changes") : t("activity.attempted")}
        >
          {outcome === "rolledBack" ? (
            <p className="text-callout text-secondary">{t("activity.nothingChanged")}</p>
          ) : null}
          <ChangeList changes={record.changes} muted={outcome !== "applied"} />
        </InspectorSection>
      ) : null}
      {record.kind === "alert" && record.error ? (
        <p className="selectable text-body">{translate(record.error)}</p>
      ) : null}
      {steps.length === 0 && leftovers.length === 0 ? null : (
        <InspectorSection title={t("activity.steps")}>
          <PlanSteps steps={steps} states={states} copyable />
          {leftovers.length > 0 ? (
            <ul className="flex flex-col gap-1">
              {leftovers.map((line) => (
                <li key={line} className="selectable text-callout text-error">
                  {line}
                </li>
              ))}
            </ul>
          ) : null}
          {script ? (
            <div>
              <Button
                size="sm"
                onClick={() =>
                  void navigator.clipboard
                    .writeText(script)
                    .then(() => toast.success(t("activity.copiedCommands")))
                }
              >
                <Copy aria-hidden className="size-3" strokeWidth={1.75} />
                {t("activity.copyCommands")}
              </Button>
            </div>
          ) : null}
        </InspectorSection>
      )}
      {checkable.length > 0 ? (
        <InspectorSection title={t("activity.checkAgain")}>
          <ul className="flex flex-col rounded-card bg-surface-inset px-3 py-1">
            {checkable.map((hostname) => (
              <CheckRow key={hostname} accountId={accountId} hostname={hostname} />
            ))}
          </ul>
        </InspectorSection>
      ) : null}
    </>
  );
}

const areas: Record<Delta["area"], { icon: LucideIcon; label: MessageKey }> = {
  route: { icon: RouteIcon, label: "activity.area.route" },
  dns: { icon: Globe, label: "activity.area.dns" },
  network: { icon: Network, label: "activity.area.network" },
  access: { icon: LockKeyhole, label: "activity.area.access" },
  loadBalancing: { icon: Split, label: "activity.area.loadBalancing" },
  snapshot: { icon: Camera, label: "activity.area.snapshot" },
  protection: { icon: Shield, label: "activity.area.protection" },
  serviceToken: { icon: Key, label: "activity.area.serviceToken" },
  worker: { icon: Inbox, label: "activity.area.worker" },
};

/** Before/after, one block per route, record or login, like a diff. */
function ChangeList({ changes, muted }: { changes: readonly Delta[]; muted: boolean }) {
  return (
    <ul className="flex flex-col gap-2">
      {changes.map((change) => {
        const { icon: Icon, label: area } = areas[change.area];
        return (
          <li
            key={`${change.area}:${change.hostname}:${change.path ?? ""}`}
            className="flex gap-2.5"
          >
            <Icon
              aria-label={t(area)}
              className="mt-0.5 size-3.5 shrink-0 text-secondary"
              strokeWidth={1.75}
            />
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="selectable truncate text-body">
                {change.hostname}
                {change.path ? <span className="text-secondary"> {change.path}</span> : null}
              </span>
              {change.before !== null ? (
                <span
                  className={cn(
                    "selectable break-all font-mono text-mono",
                    muted ? "text-secondary" : "text-error",
                  )}
                >
                  <span aria-hidden>− </span>
                  <span className="sr-only">{t("activity.before")}</span>
                  {translate(change.before)}
                </span>
              ) : null}
              {change.after !== null ? (
                <span
                  className={cn(
                    "selectable break-all font-mono text-mono",
                    muted ? "text-secondary" : "text-healthy",
                  )}
                >
                  <span aria-hidden>+ </span>
                  <span className="sr-only">{t("activity.after")}</span>
                  {translate(change.after)}
                </span>
              ) : null}
            </div>
          </li>
        );
      })}
    </ul>
  );
}

/** A hostname and a button that checks it end to end, with the result. */
function CheckRow({ accountId, hostname }: { accountId: string; hostname: string }) {
  const verify = useVerify(accountId);
  const result = verify.data;
  const status: Status = verify.isPending
    ? "connecting"
    : !result
      ? "idle"
      : result.failure
        ? "error"
        : "healthy";
  const message = verify.isPending
    ? t("activity.check.checking")
    : verify.error
      ? toIpcError(verify.error).message
      : result
        ? result.message
          ? translate(result.message)
          : result.status
            ? t("activity.check.worksStatus", { status: String(result.status) })
            : t("activity.check.works")
        : null;
  return (
    <li className="flex min-h-9 items-center gap-2.5 border-inset border-b-hairline py-1.5 last:border-b-0">
      <StatusDot status={status} label={message ?? t("activity.check.notChecked")} />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="selectable truncate text-body">{hostname}</span>
        {message ? (
          <span className="text-callout text-secondary" aria-live="polite">
            {message}
          </span>
        ) : null}
      </div>
      <Button
        size="sm"
        pending={verify.isPending}
        onClick={() => verify.mutate({ hostname, wait: false })}
      >
        {t("activity.check.check")}
      </Button>
    </li>
  );
}

/** Entries from before the structured record: the step lines as they were logged. */
function PlainSteps({ lines }: { lines: readonly string[] }) {
  return (
    <InspectorSection title={t("activity.steps")}>
      {lines.length === 0 ? (
        <p className="text-callout text-secondary">{t("activity.noDetails")}</p>
      ) : (
        <ol className="flex flex-col gap-1.5">
          {lines.map((line, index) => (
            <li
              // biome-ignore lint/suspicious/noArrayIndexKey: lines are positional
              key={index}
              className={
                line.startsWith("Failed") || line.startsWith("Left over")
                  ? "selectable text-callout text-error"
                  : "selectable text-callout"
              }
            >
              {line}
            </li>
          ))}
        </ol>
      )}
    </InspectorSection>
  );
}
