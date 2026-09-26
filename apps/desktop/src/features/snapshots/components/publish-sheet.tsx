import { useEffect, useId, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import { missingNeeds, PermissionFix, useCapabilities } from "@/features/accounts";
import { PlanSteps } from "@/features/routes";
import { t } from "@/lib/i18n";
import type { PlanView, PreparedView, SnapshotChange, ZoneRef } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import {
  chooseFolder,
  useDetectProject,
  usePrepare,
  useSnapshotApply,
  useSnapshotPreview,
} from "../queries";
import { type DetailsState, DetailsStep } from "./details-step";
import { FailedOutcome, transferring, UploadProgress } from "./publish-progress";
import {
  changeFor,
  collectVars,
  initialDetails,
  initialSource,
  type PublishMode,
  permissionNeeds,
  sourceReady,
} from "./publish-state";
import { buildCommand, type SourceState, SourceStep } from "./source-step";

export type { PublishMode } from "./publish-state";

type Stage = "source" | "details" | "review" | "applying";

const PERMISSION = "core.error.cloudflare.permission";

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
    const vars = collectVars(source);
    if (!vars) {
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
    const next = mode && changeFor(mode, details, prepared, zones);
    if (next) review(next);
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

  const needs = permissionNeeds(details, zones, updating);
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
  const transfer = apply.isPending ? transferring(apply.steps) : undefined;

  const footer = (() => {
    switch (stage) {
      case "source":
        return (
          <>
            <Button onClick={onClose}>{t("common.cancel")}</Button>
            <Button
              variant="primary"
              pending={prepare.isPending}
              disabled={!sourceReady(source)}
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
          {transfer ? <UploadProgress transfer={transfer} /> : null}
          {outcome && outcome.type !== "applied" ? <FailedOutcome outcome={outcome} /> : null}
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
