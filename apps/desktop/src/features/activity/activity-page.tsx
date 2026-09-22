import { Activity, RefreshCw } from "lucide-react";
import { useState } from "react";
import { useUiStore } from "@/app/ui-store";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { useAccounts, useActiveAccount } from "@/features/accounts";
import { useActivity } from "@/features/routes/queries";
import type { ActivityEntry } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";

const outcomes: Record<string, { dot: Status; label: string }> = {
  applied: { dot: "healthy", label: "Applied" },
  rolledBack: { dot: "warning", label: "Failed and undone" },
  partiallyApplied: { dot: "error", label: "Failed, partly undone" },
};

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
  const entries = activity.data ?? [];
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
  ) : entries.length === 0 || (isSuccess && !active) ? (
    <EmptyState
      icon={Activity}
      title="No activity yet"
      description="Every change Teitunnel makes to Cloudflare is recorded here, step by step."
    />
  ) : (
    <SplitView
      id="activity"
      list={
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
        />
      }
    >
      {selected ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <Inspector
            title={selected.summary}
            subtitle={`${day(selected.at)} at ${time(selected.at)} · ${outcomeOf(selected).label}`}
          >
            <InspectorSection title="Steps">
              {selected.detail.length === 0 ? (
                <p className="text-callout text-secondary">No details were recorded.</p>
              ) : (
                <ol className="flex flex-col gap-1.5">
                  {selected.detail.map((line, index) => (
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
