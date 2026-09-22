import { CircleCheck, RefreshCw, Stethoscope } from "lucide-react";
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
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet } from "@/features/accounts";
import { useInstallBinary } from "@/features/binary/queries";
import { RouteSheet, type SheetMode, useKeepTheirs, useTunnelAction } from "@/features/routes";
import type { Fix, Issue, Severity } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { hasSafeCandidates, useFixSafe, useIssues } from "./queries";

const severities: Record<Severity, { dot: Status; group: string; label: string }> = {
  error: { dot: "error", group: "Problems", label: "Problem" },
  warning: { dot: "warning", group: "Warnings", label: "Warning" },
  info: { dot: "idle", group: "Suggestions", label: "Suggestion" },
};

function fixLabel(fix: Fix): string {
  switch (fix.type) {
    case "change":
      return `${fix.label}…`;
    case "installBinary":
      return "Install cloudflared";
    case "startConnector":
      return "Start Connector";
    case "keepTheirs":
      return "Keep Changes";
    case "reconnect":
      return "Connect Again…";
  }
}

/** A fix that runs directly (no plan to review). */
function FixButton({ fix, primary }: { fix: Fix; primary: boolean }) {
  const install = useInstallBinary();
  const accountId = fix.type === "startConnector" || fix.type === "keepTheirs" ? fix.accountId : "";
  const connector = useTunnelAction(accountId);
  const keep = useKeepTheirs(accountId);
  const variant = primary ? "primary" : "secondary";
  const failed = (error: unknown) => toast.error(toIpcError(error).message);
  switch (fix.type) {
    case "reconnect":
      return <ConnectSheet trigger={<Button variant={variant}>{fixLabel(fix)}</Button>} />;
    case "installBinary":
      return (
        <Button
          variant={variant}
          disabled={install.isPending}
          onClick={() => install.mutate(undefined, { onError: failed })}
        >
          {install.isPending ? "Installing…" : fixLabel(fix)}
        </Button>
      );
    case "startConnector":
      return (
        <Button
          variant={variant}
          disabled={connector.isPending}
          onClick={() => connector.mutate({ action: "start", tunnelId: "" }, { onError: failed })}
        >
          {fixLabel(fix)}
        </Button>
      );
    case "keepTheirs":
      return (
        <Button
          variant={variant}
          disabled={keep.isPending}
          onClick={() => keep.mutate(undefined, { onError: failed })}
        >
          {fixLabel(fix)}
        </Button>
      );
    case "change":
      return null;
  }
}

function IssueInspector({
  issue,
  onReview,
}: {
  issue: Issue;
  onReview: (mode: SheetMode, accountId: string) => void;
}) {
  const setIgnored = useUiStore((state) => state.setIgnored);
  const severity = severities[issue.severity];
  return (
    <Inspector
      title={issue.title}
      subtitle={
        <span className="flex items-center gap-1.5">
          <StatusDot status={severity.dot} label={severity.label} /> {issue.subject}
        </span>
      }
      actions={
        <>
          {issue.fixes.map((fix, index) =>
            fix.type === "change" && issue.accountId ? (
              <Button
                key={fix.label}
                variant={index === 0 ? "primary" : "secondary"}
                onClick={() =>
                  issue.accountId &&
                  onReview({ kind: "fix", change: fix.change, label: fix.label }, issue.accountId)
                }
              >
                {fixLabel(fix)}
              </Button>
            ) : (
              <FixButton key={fix.type} fix={fix} primary={index === 0} />
            ),
          )}
          <Button variant="plain" onClick={() => setIgnored(issue.id, true)}>
            Ignore
          </Button>
        </>
      }
    >
      <p className="selectable text-body">{issue.detail}</p>
      {issue.evidence.length > 0 ? (
        <InspectorSection title="Details">
          <ul className="flex flex-col gap-1">
            {issue.evidence.map((line) => (
              <li key={line} className="selectable break-all font-mono text-mono text-secondary">
                {line}
              </li>
            ))}
          </ul>
        </InspectorSection>
      ) : null}
    </Inspector>
  );
}

/** Problems Teitunnel found, worst first, each with a fix or clear guidance. */
export function DoctorPage() {
  const doctor = useIssues();
  const { issues, ignoredCount } = doctor;
  const ignored = useUiStore((state) => state.ignoredIssues);
  const setIgnored = useUiStore((state) => state.setIgnored);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sheet, setSheet] = useState<{ mode: SheetMode; accountId: string } | null>(null);
  const selected = issues.find((i) => i.id === selectedId) ?? issues[0] ?? null;

  const fixSafe = useFixSafe();
  const runSafeFixes = () =>
    fixSafe.mutate(undefined, {
      onSuccess: (report) => {
        const left = report.skipped > 0 ? ` ${report.skipped} need your review.` : "";
        if (report.failed.length > 0) {
          toast.error(`Fixed ${report.fixed}; ${report.failed.length} couldn't be fixed.`, {
            description: report.failed.join("\n"),
          });
        } else {
          toast.success(`Fixed ${report.fixed} issue${report.fixed === 1 ? "" : "s"}.${left}`);
        }
      },
      onError: (error) => toast.error(toIpcError(error).message),
    });

  const toolbar = (
    <TitlebarToolbar title="Doctor">
      {hasSafeCandidates(issues) ? (
        <Button size="sm" disabled={fixSafe.isPending} onClick={runSafeFixes}>
          {fixSafe.isPending ? "Fixing…" : "Fix Safe Issues"}
        </Button>
      ) : null}
      <IconButton
        icon={RefreshCw}
        label="Check again"
        onClick={() => void doctor.refetch()}
        disabled={doctor.isFetching}
      />
    </TitlebarToolbar>
  );

  const body = (() => {
    if (doctor.error) {
      const error = toIpcError(doctor.error);
      return (
        <ErrorState
          title="Couldn't run the checks"
          message={error.message}
          hint={error.hint}
          action={<Button onClick={() => void doctor.refetch()}>Try again</Button>}
        />
      );
    }
    if (doctor.isPending) {
      return (
        <div className="flex flex-col gap-2 p-3">
          <Skeleton className="h-11" />
          <Skeleton className="h-11" />
        </div>
      );
    }
    if (issues.length === 0) {
      return (
        <EmptyState
          icon={ignoredCount > 0 ? Stethoscope : CircleCheck}
          title="Everything looks good"
          description={
            ignoredCount > 0
              ? `No new problems. ${ignoredCount} ignored issue${ignoredCount === 1 ? " is" : "s are"} hidden.`
              : "Teitunnel checks cloudflared, your domains, DNS records and connectors every few minutes."
          }
          action={
            ignoredCount > 0 ? (
              <Button
                onClick={() => {
                  for (const id of ignored) setIgnored(id, false);
                }}
              >
                Show Ignored Issues
              </Button>
            ) : undefined
          }
        />
      );
    }
    return (
      <SplitView
        id="doctor"
        list={
          <ListPane
            label="Issues"
            items={issues}
            getId={(issue) => issue.id}
            groupOf={(issue) => severities[issue.severity].group}
            selectedId={selected?.id ?? null}
            onSelect={setSelectedId}
            renderRow={(issue) => (
              <ListRow
                title={issue.title}
                subtitle={issue.subject}
                leading={
                  <StatusDot
                    status={severities[issue.severity].dot}
                    label={severities[issue.severity].label}
                  />
                }
              />
            )}
          />
        }
      >
        {selected ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <IssueInspector
              issue={selected}
              onReview={(mode, accountId) => setSheet({ mode, accountId })}
            />
          </div>
        ) : null}
      </SplitView>
    );
  })();

  return (
    <>
      {toolbar}
      {body}
      <RouteSheet
        accountId={sheet?.accountId ?? ""}
        zones={[]}
        mode={sheet?.mode ?? null}
        onClose={() => {
          setSheet(null);
          void doctor.refetch();
        }}
      />
    </>
  );
}
