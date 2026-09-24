import { useEffect, useId, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ProgressBar } from "@/components/ui/progress-bar";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import {
  missingNeeds,
  PermissionFix,
  type PermissionNeed,
  useCapabilities,
} from "@/features/accounts";
import { joinHostname, PlanSteps, parseAllowed } from "@/features/routes";
import { t, translate } from "@/lib/i18n";
import type {
  PlanView,
  PreparedView,
  SnapshotChange,
  SnapshotOptions,
  SnapshotView,
  ZoneRef,
} from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { formatBytes } from "../format";
import {
  chooseFolder,
  useDetectProject,
  usePrepare,
  useSnapshotApply,
  useSnapshotPreview,
} from "../queries";
import { type DetailsState, DetailsStep } from "./details-step";
import { buildCommand, type SourceState, SourceStep } from "./source-step";

/** What the sheet does: publish a new Snapshot, or a new version of one. */
export type PublishMode =
  | { kind: "new"; accountId: string; siteUrl?: string }
  | { kind: "update"; snapshot: SnapshotView };

type Stage = "source" | "details" | "review" | "applying";

const PERMISSION = "core.error.cloudflare.permission";

function initialSource(mode: PublishMode): SourceState {
  if (mode.kind === "new") {
    return mode.siteUrl
      ? { kind: "site", folder: null, project: null, url: mode.siteUrl }
      : { kind: "folder", folder: null, project: null, url: "http://localhost:5173" };
  }
  const source = mode.snapshot.source;
  switch (source?.type) {
    case "folder":
      return { kind: "folder", folder: source.path, project: null, url: "" };
    case "crawl":
      return { kind: "site", folder: null, project: null, url: source.url };
    default:
      return { kind: "keep", folder: null, project: null, url: "" };
  }
}

function initialDetails(mode: PublishMode, zones: readonly ZoneRef[]): DetailsState {
  const snapshot = mode.kind === "update" ? mode.snapshot : null;
  return {
    name: "",
    address: zones.length > 0 ? "domain" : "workersDev",
    hostname: joinHostname("preview", zones[0]?.name ?? ""),
    protection: snapshot?.access ? "login" : snapshot?.password ? "password" : "none",
    password: "",
    allowed: snapshot?.access
      ? [...snapshot.access.emails, ...snapshot.access.emailDomains.map((d) => `@${d}`)].join(", ")
      : "",
    spa: snapshot?.spa ?? false,
    expires: "never",
    comments: snapshot?.comments ?? false,
  };
}

interface PublishSheetProps {
  mode: PublishMode | null;
  zones: readonly ZoneRef[];
  onClose: () => void;
  /** A Snapshot was published (its name), to select it. */
  onPublished: (name: string) => void;
}

/**
 * Publishing, like every Cloudflare change: choose the files → settings → review the
 * plan → apply with live progress (the upload included). Nothing changes before Publish.
 */
export function PublishSheet({ mode, zones, onClose, onPublished }: PublishSheetProps) {
  const accountId = mode?.kind === "update" ? mode.snapshot.accountId : (mode?.accountId ?? "");
  const updating = mode?.kind === "update";
  const [stage, setStage] = useState<Stage>("source");
  const [source, setSource] = useState<SourceState>(() =>
    initialSource(mode ?? { kind: "new", accountId: "" }),
  );
  const [details, setDetails] = useState<DetailsState>(() =>
    initialDetails(mode ?? { kind: "new", accountId: "" }, zones),
  );
  const [prepared, setPrepared] = useState<PreparedView | null>(null);
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [change, setChange] = useState<SnapshotChange | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const detect = useDetectProject();
  const prepare = usePrepare();
  const preview = useSnapshotPreview(accountId);
  const apply = useSnapshotApply(accountId);
  const caps = useCapabilities(accountId).data;
  const confirmId = useId();

  // Start over whenever the sheet opens.
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs once per opening
  useEffect(() => {
    if (!mode) return;
    setStage("source");
    setSource(initialSource(mode));
    setDetails(initialDetails(mode, zones));
    setPrepared(null);
    setPlan(null);
    setChange(null);
    setConfirmed(false);
    detect.reset();
    prepare.reset();
    preview.reset();
    apply.reset();
  }, [mode]);

  const pickFolder = async () => {
    const folder = await chooseFolder();
    if (!folder) return;
    setPrepared(null);
    if (source.kind === "build") {
      detect.mutate(folder, { onSuccess: (project) => setSource({ ...source, project }) });
    } else {
      setSource({ ...source, folder });
    }
  };

  const collect = () => {
    const vars =
      source.kind === "site"
        ? ({ kind: "site", url: source.url } as const)
        : source.kind === "build" && source.project
          ? ({ kind: "build", dir: source.project.dir } as const)
          : source.folder
            ? ({ kind: "folder", path: source.folder } as const)
            : null;
    if (source.kind === "keep" || !vars) {
      setStage("details");
      return;
    }
    prepare.mutate(vars, {
      onSuccess: (result) => {
        setPrepared(result);
        setDetails((current) => ({
          ...current,
          name: current.name || result.suggestedName,
          spa: current.spa || result.singlePage,
        }));
        setStage("details");
      },
    });
  };

  const options = (): SnapshotOptions => ({
    spa: details.spa,
    password:
      details.protection !== "password"
        ? { type: "remove" }
        : details.password
          ? { type: "set", password: details.password }
          : { type: "keep" },
    access: details.protection === "login" ? parseAllowed(details.allowed) : null,
    expiresInDays: details.expires === "never" ? null : Number(details.expires),
    comments: details.comments,
  });

  const review = (next: SnapshotChange) => {
    setChange(next);
    setConfirmed(false);
    preview.mutate(next, {
      onSuccess: (result) => {
        setPlan(result);
        setStage("review");
      },
    });
  };

  const submitDetails = () => {
    if (!mode) return;
    if (mode.kind === "update") {
      review({
        type: "update",
        snapshot: mode.snapshot.id,
        prepared: prepared?.id ?? null,
        options: options(),
      });
    } else if (prepared) {
      review({
        type: "publish",
        prepared: prepared.id,
        name: details.name,
        address:
          details.address === "domain" && zones.length > 0
            ? { type: "domain", hostname: details.hostname }
            : { type: "workersDev" },
        options: options(),
      });
    }
  };

  const runApply = () => {
    if (!plan || !change) return;
    setStage("applying");
    apply.mutate(
      { change, fingerprint: plan.fingerprint, confirmed },
      {
        onSuccess: (outcome) => {
          if (outcome.type !== "applied") return;
          const name = mode?.kind === "update" ? mode.snapshot.name : details.name;
          toast.success(
            t(updating ? "snapshots.sheet.updated" : "snapshots.sheet.published", { name }),
          );
          onPublished(name);
          onClose();
        },
        onError: (error) => {
          if (toIpcError(error).code === "conflict") review(change);
          else setStage("review");
        },
      },
    );
  };

  const zone = zones.find(
    (z) => details.hostname === z.name || details.hostname.endsWith(`.${z.name}`),
  );
  const needs: PermissionNeed[] = [
    { kind: "workers" },
    ...(!updating && details.address === "domain" && zone
      ? [{ kind: "workersRoutes" as const, zone: zone.name }]
      : []),
    ...(details.protection === "login" ? [{ kind: "access" as const }] : []),
  ];
  const failure = preview.error
    ? toIpcError(preview.error)
    : apply.error
      ? toIpcError(apply.error)
      : null;
  const gaps = caps ? missingNeeds(caps, needs) : [];
  const fieldError = (field: string) =>
    preview.error && toIpcError(preview.error).field === field
      ? toIpcError(preview.error).message
      : null;
  const generalError =
    failure &&
    failure.key !== PERMISSION &&
    !["name", "hostname", "password", "access"].includes(failure.field ?? "")
      ? failure.message
      : null;
  const outcome = apply.data;
  const transfer = Object.values(apply.steps).find((s) => s.state === "transferring");

  const footer = (() => {
    switch (stage) {
      case "source":
        return (
          <>
            <Button onClick={onClose}>{t("common.cancel")}</Button>
            <Button
              variant="primary"
              pending={prepare.isPending}
              disabled={
                (source.kind === "folder" && !source.folder) ||
                (source.kind === "build" && !source.project) ||
                (source.kind === "site" && !source.url.trim())
              }
              onClick={collect}
            >
              {source.kind === "build" && source.project && buildCommand(source.project)
                ? t("snapshots.sheet.buildAndContinue")
                : source.kind === "site"
                  ? t("snapshots.sheet.capture")
                  : t("snapshots.sheet.continue")}
            </Button>
          </>
        );
      case "details":
        return (
          <>
            <Button onClick={() => setStage("source")}>{t("snapshots.sheet.back")}</Button>
            <Button
              variant="primary"
              pending={preview.isPending}
              disabled={gaps.length > 0 || (!updating && !details.name.trim())}
              onClick={submitDetails}
            >
              {t("snapshots.sheet.review")}
            </Button>
          </>
        );
      case "review":
        return (
          <>
            <Button onClick={() => setStage("details")}>{t("snapshots.sheet.back")}</Button>
            <Button
              variant="primary"
              disabled={
                !plan || plan.steps.length === 0 || (plan.requiresConfirmation && !confirmed)
              }
              onClick={runApply}
            >
              {updating ? t("snapshots.sheet.publishVersion") : t("snapshots.sheet.publish")}
            </Button>
          </>
        );
      case "applying":
        return (
          <Button variant="primary" pending={apply.isPending} onClick={onClose}>
            {t("common.close")}
          </Button>
        );
    }
  })();

  return (
    <Sheet open={mode !== null} onOpenChange={(open) => !open && !apply.isPending && onClose()}>
      <SheetContent
        width="lg"
        title={
          mode?.kind === "update"
            ? t("snapshots.sheet.updateTitle", { name: mode.snapshot.name })
            : t("snapshots.sheet.title")
        }
        description={t("snapshots.sheet.description")}
        footer={footer}
      >
        <div className="flex flex-col gap-4">
          {stage === "source" ? (
            <SourceStep
              state={source}
              onChange={(next) => {
                setSource(next);
                setPrepared(null);
                prepare.reset();
              }}
              canKeep={updating}
              onChooseFolder={() => void pickFolder()}
              detecting={detect.isPending}
              prepared={prepared}
              output={prepare.output}
              error={
                prepare.error
                  ? toIpcError(prepare.error).message
                  : detect.error
                    ? toIpcError(detect.error).message
                    : null
              }
            />
          ) : null}
          {stage === "details" ? (
            <DetailsStep
              state={details}
              onChange={setDetails}
              zones={zones}
              updating={updating}
              hasPassword={mode?.kind === "update" && mode.snapshot.password}
              fieldError={fieldError}
            />
          ) : null}
          {(stage === "review" || stage === "applying") && plan ? (
            <PlanSteps
              steps={plan.steps}
              warnings={plan.warnings}
              {...(stage === "applying" ? { states: apply.steps } : {})}
            />
          ) : null}
          {stage === "review" && plan?.requiresConfirmation ? (
            <div className="flex items-center gap-2 text-body">
              <Checkbox
                id={confirmId}
                checked={confirmed}
                onCheckedChange={(v) => setConfirmed(v === true)}
              />
              <label htmlFor={confirmId}>{t("snapshots.sheet.confirmReplace")}</label>
            </div>
          ) : null}
          {stage === "review" && plan?.steps.length === 0 ? (
            <p className="text-callout text-secondary">{t("snapshots.sheet.nothing")}</p>
          ) : null}
          {transfer?.state === "transferring" && apply.isPending ? (
            <div className="flex flex-col gap-1">
              <ProgressBar
                label={t("snapshots.sheet.uploading")}
                value={
                  (transfer.totalBytes ?? 0) === 0
                    ? 1
                    : (transfer.bytes ?? 0) / (transfer.totalBytes ?? 1)
                }
              />
              <p className="text-callout text-secondary tabular">
                {t("snapshots.sheet.uploadProgress", {
                  files: transfer.files ?? 0,
                  total: transfer.totalFiles ?? 0,
                  size: formatBytes(transfer.totalBytes ?? 0),
                })}
              </p>
            </div>
          ) : null}
          {outcome && outcome.type !== "applied" ? (
            <p role="alert" className="text-callout text-error">
              {outcome.type === "rolledBack"
                ? t("snapshots.sheet.rolledBack", { error: translate(outcome.error) })
                : t("snapshots.sheet.partial", {
                    error: translate(outcome.error),
                    leftovers: outcome.leftovers.map(translate).join("; "),
                  })}
            </p>
          ) : null}
          {stage === "details" && gaps.length > 0 ? (
            <PermissionFix accountId={accountId} needs={needs} />
          ) : null}
          {failure?.key === PERMISSION ? (
            <PermissionFix accountId={accountId} needs={needs} refused onReady={submitDetails} />
          ) : null}
          {generalError && stage !== "source" ? (
            <p role="alert" className="text-callout text-error">
              {generalError}
            </p>
          ) : null}
        </div>
      </SheetContent>
    </Sheet>
  );
}
