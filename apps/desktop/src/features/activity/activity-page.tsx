import { Activity, Copy, Globe, RefreshCw, Route as RouteIcon } from "lucide-react";
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
import type { ActivityEntry, ActivityRecord, Delta } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { commandScript, matches, type Show, showOptions } from "./model";

const outcomes: Record<string, { dot: Status; label: string }> = {
  applied: { dot: "healthy", label: "Applied" },
  rolledBack: { dot: "warning", label: "Failed and undone" },
  partiallyApplied: { dot: "error", label: "Failed, partly undone" },
};

/** The Domain menu's "any" item (Radix Select items can't have an empty value). */
const ANY_DOMAIN = "*";

function outcomeOf(entry: ActivityEntry) {
  return outcomes[entry.outcome] ?? { dot: "idle" as const, label: entry.outcome };
}

function day(at: number | null) {
  if (at === null) return "Earlier";
  const date = new Date(at);
  const today = new Date();
  const yesterday = new Date(today.getTime() - 86_400_000);
  if (date.toDateString() === today.toDateString()) return "Today";
  if (date.toDateString() === yesterday.toDateString()) return "Yesterday";
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
    <TitlebarToolbar title="Activity">
      {accounts.length > 1 && active ? (
        <Select
          label="Account"
          options={accounts.map((a) => ({ value: a.id, label: a.name }))}
          value={active.id}
          onValueChange={setActive}
        />
      ) : null}
      {active ? (
        <IconButton
          icon={RefreshCw}
          label="Refresh activity"
          onClick={() => void activity.refetch()}
          disabled={activity.isFetching}
        />
      ) : null}
    </TitlebarToolbar>
  );

  const body = activity.error ? (
    <ErrorState
      title="Couldn't load activity"
      message={toIpcError(activity.error).message}
      action={<Button onClick={() => void activity.refetch()}>Try again</Button>}
    />
  ) : activity.isPending && active ? (
    <div className="flex flex-col gap-2 p-3">
      <Skeleton className="h-11" />
    </div>
  ) : all.length === 0 || (isSuccess && !active) ? (
    <EmptyState
      icon={Activity}
      title="No activity yet"
      description="Every change Teitunnel makes to Cloudflare is recorded here, step by step."
    />
  ) : (
    <SplitView
      id="activity"
      list={
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="flex flex-col gap-1.5 px-2.5 pt-1 pb-1.5">
            <Input
              type="search"
              aria-label="Filter activity"
              placeholder="Filter"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              className="rounded-full"
            />
            <div className="flex gap-1.5">
              <Select
                label="Show"
                options={showOptions}
                value={show}
                onValueChange={setShow}
                className="min-w-0 flex-1"
              />
              {zones.length > 1 ? (
                <Select
                  label="Domain"
                  options={[
                    { value: ANY_DOMAIN, label: "All Domains" },
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
            label="Activity"
            items={entries}
            getId={(e) => String(e.id)}
            groupOf={(e) => day(e.at)}
            selectedId={selected ? String(selected.id) : null}
            onSelect={setSelectedId}
            renderRow={(entry) => (
              <ListRow
                title={entry.summary}
                subtitle={`${time(entry.at)} · ${outcomeOf(entry).label}`}
                leading={<StatusDot status={outcomeOf(entry).dot} label={outcomeOf(entry).label} />}
              />
            )}
            empty={<EmptyState title="No matches" description="No changes match the filter." />}
          />
        </div>
      }
    >
      {selected && active ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <Inspector
            title={selected.summary}
            subtitle={`${day(selected.at)} at ${time(selected.at)} · ${outcomeOf(selected).label}`}
          >
            {selected.record ? (
              <RecordDetails
                accountId={active.id}
                outcome={selected.outcome}
                record={selected.record}
                leftovers={selected.detail.filter((line) => line.startsWith("Left over"))}
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
        <InspectorSection title={outcome === "applied" ? "Changes" : "Attempted Changes"}>
          {outcome === "rolledBack" ? (
            <p className="text-callout text-secondary">
              Nothing was changed: every step was undone.
            </p>
          ) : null}
          <ChangeList changes={record.changes} muted={outcome !== "applied"} />
        </InspectorSection>
      ) : null}
      <InspectorSection title="Steps">
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
                  .then(() => toast.success("Copied the commands"))
              }
            >
              <Copy aria-hidden className="size-3" strokeWidth={1.75} />
              Copy All as Commands
            </Button>
          </div>
        ) : null}
      </InspectorSection>
      {checkable.length > 0 ? (
        <InspectorSection title="Check Again">
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

/** Before/after, one block per route or record, like a diff. */
function ChangeList({ changes, muted }: { changes: readonly Delta[]; muted: boolean }) {
  return (
    <ul className="flex flex-col gap-2">
      {changes.map((change) => {
        const Icon = change.area === "route" ? RouteIcon : Globe;
        const area = change.area === "route" ? "Route" : "DNS record";
        return (
          <li
            key={`${change.area}:${change.hostname}:${change.path ?? ""}`}
            className="flex gap-2.5"
          >
            <Icon
              aria-label={area}
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
                  <span className="sr-only">Before: </span>
                  {change.before}
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
                  <span className="sr-only">After: </span>
                  {change.after}
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
    ? "Checking…"
    : verify.error
      ? toIpcError(verify.error).message
      : result
        ? (result.message ?? `Works${result.status ? ` (HTTP ${result.status})` : ""}`)
        : null;
  return (
    <li className="flex min-h-9 items-center gap-2.5 border-inset border-b-hairline py-1.5 last:border-b-0">
      <StatusDot status={status} label={message ?? "Not checked"} />
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
        disabled={verify.isPending}
        onClick={() => verify.mutate({ hostname, wait: false })}
      >
        Check
      </Button>
    </li>
  );
}

/** Entries from before the structured record: the step lines as they were logged. */
function PlainSteps({ lines }: { lines: readonly string[] }) {
  return (
    <InspectorSection title="Steps">
      {lines.length === 0 ? (
        <p className="text-callout text-secondary">No details were recorded.</p>
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
