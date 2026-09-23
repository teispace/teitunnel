import { CircleCheck, RefreshCw, Stethoscope } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
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
import { t, translate } from "@/lib/i18n";
import type { Fix, Issue, Severity } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { DiagnosticsDialog } from "./diagnostics-dialog";
import { hasSafeCandidates, useFixSafe, useIssues, useSetIgnored } from "./queries";

const dots: Record<Severity, Status> = { error: "error", warning: "warning", info: "idle" };

const severityOf = (severity: Severity) => ({
  dot: dots[severity],
  group: t(`doctor.severity.${severity}.group`),
  label: t(`doctor.severity.${severity}.label`),
});

function fixLabel(fix: Fix): string {
  switch (fix.type) {
    case "change":
      return t("doctor.fix.change", { label: translate(fix.label) });
    case "installBinary":
      return t("doctor.fix.installBinary");
    case "startConnector":
      return t("doctor.fix.startConnector");
    case "keepTheirs":
      return t("doctor.fix.keepTheirs");
    case "reconnect":
      return t("doctor.fix.reconnect");
    case "cleanConnections":
      return t("doctor.fix.cleanConnections");
  }
}

/** A fix that runs directly (no plan to review). */
function FixButton({ fix, primary }: { fix: Fix; primary: boolean }) {
  const install = useInstallBinary();
  const accountId =
    fix.type === "startConnector" || fix.type === "keepTheirs" || fix.type === "cleanConnections"
      ? fix.accountId
      : "";
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
          {install.isPending ? t("doctor.installing") : fixLabel(fix)}
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
    case "cleanConnections":
      return (
        <Button
          variant={variant}
          disabled={connector.isPending}
          onClick={() =>
            connector.mutate({ action: "clean", tunnelId: fix.tunnelId }, { onError: failed })
          }
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
  const setIgnored = useSetIgnored();
  const severity = severityOf(issue.severity);
  return (
    <Inspector
      title={translate(issue.title)}
      subtitle={
        <span className="flex items-center gap-1.5">
          <StatusDot status={severity.dot} label={severity.label} /> {translate(issue.label)}
        </span>
      }
      actions={
        <>
          {issue.fixes.map((fix, index) =>
            fix.type === "change" && issue.accountId ? (
              <Button
                key={fix.label.key}
                variant={index === 0 ? "primary" : "secondary"}
                onClick={() =>
                  issue.accountId &&
                  onReview(
                    { kind: "fix", change: fix.change, label: translate(fix.label) },
                    issue.accountId,
                  )
                }
              >
                {fixLabel(fix)}
              </Button>
            ) : (
              <FixButton key={fix.type} fix={fix} primary={index === 0} />
            ),
          )}
          <Button
            variant="plain"
            disabled={setIgnored.isPending}
            onClick={() => setIgnored.mutate({ ids: [issue.id], ignored: true })}
          >
            {t("doctor.ignore")}
          </Button>
        </>
      }
    >
      <p className="selectable text-body">{translate(issue.detail)}</p>
      {issue.evidence.length > 0 ? (
        <InspectorSection title={t("doctor.details")}>
          <ul className="flex flex-col gap-1">
            {issue.evidence.map((line) => {
              const text = translate(line);
              return (
                <li key={text} className="selectable break-all font-mono text-mono text-secondary">
                  {text}
                </li>
              );
            })}
          </ul>
        </InspectorSection>
      ) : null}
    </Inspector>
  );
}

/** Problems Teitunnel found, worst first, each with a fix or clear guidance. */
export function DoctorPage() {
  const doctor = useIssues();
  const { issues, ignoredCount, ignoredIds } = doctor;
  const setIgnored = useSetIgnored();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sheet, setSheet] = useState<{ mode: SheetMode; accountId: string } | null>(null);
  const selected = issues.find((i) => i.id === selectedId) ?? issues[0] ?? null;

  const fixSafe = useFixSafe();
  const runSafeFixes = () =>
    fixSafe.mutate(undefined, {
      onSuccess: (report) => {
        const left =
          report.skipped > 0 ? ` ${t("doctor.needReview", { count: report.skipped })}` : "";
        if (report.failed.length > 0) {
          toast.error(
            t("doctor.fixedSome", { fixed: report.fixed, failed: report.failed.length }),
            {
              description: report.failed.join("\n"),
            },
          );
        } else {
          toast.success(`${t("doctor.fixed", { count: report.fixed })}${left}`);
        }
      },
      onError: (error) => toast.error(toIpcError(error).message),
    });

  const toolbar = (
    <TitlebarToolbar title={t("doctor.title")}>
      {hasSafeCandidates(issues) ? (
        <Button size="sm" disabled={fixSafe.isPending} onClick={runSafeFixes}>
          {fixSafe.isPending ? t("doctor.fixing") : t("doctor.fixSafe")}
        </Button>
      ) : null}
      <DiagnosticsDialog />
      <IconButton
        icon={RefreshCw}
        label={t("doctor.checkAgain")}
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
          title={t("doctor.runFailed")}
          message={error.message}
          hint={error.hint}
          action={<Button onClick={() => void doctor.refetch()}>{t("common.tryAgain")}</Button>}
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
          title={t("doctor.allGood")}
          description={
            ignoredCount > 0
              ? t("doctor.ignoredHidden", { count: ignoredCount })
              : t("doctor.checksDescription")
          }
          action={
            ignoredCount > 0 ? (
              <Button onClick={() => setIgnored.mutate({ ids: ignoredIds, ignored: false })}>
                {t("doctor.showIgnored")}
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
            label={t("doctor.issues")}
            items={issues}
            getId={(issue) => issue.id}
            groupOf={(issue) => severityOf(issue.severity).group}
            selectedId={selected?.id ?? null}
            onSelect={setSelectedId}
            renderRow={(issue) => (
              <ListRow
                title={translate(issue.title)}
                subtitle={translate(issue.label)}
                leading={
                  <StatusDot
                    status={severityOf(issue.severity).dot}
                    label={severityOf(issue.severity).label}
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
