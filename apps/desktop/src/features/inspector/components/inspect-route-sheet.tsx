import { useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { PlanSteps } from "@/features/routes/components/plan-steps";
import { t, translate } from "@/lib/i18n";
import type { InspectPlan, Outcome } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { type InspectRouteTarget, useInspectRouteApply, useInspectRoutePreview } from "../queries";

type Stage = "review" | "applying";

interface InspectRouteSheetProps {
  /** What to plan (`null`: closed). */
  target: InspectRouteTarget | null;
  /** Ending an inspection left behind by an inspector that's gone. */
  restore?: boolean;
  onClose: () => void;
}

/**
 * Inspect this route / Stop Inspecting / Restore the Route: the plan that points the
 * route at the inspector on this computer (or back at its own service), reviewed like any
 * other change, then applied with live progress.
 */
export function InspectRouteSheet({ target, restore = false, onClose }: InspectRouteSheetProps) {
  const [stage, setStage] = useState<Stage>("review");
  const [plan, setPlan] = useState<InspectPlan | null>(null);
  const [empty, setEmpty] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const preview = useInspectRoutePreview();
  const apply = useInspectRouteApply();
  const navigate = useNavigate();
  const open = target !== null;

  const review = (next: InspectRouteTarget) => {
    setStage("review");
    setPlan(null);
    setEmpty(false);
    setOutcome(null);
    apply.reset();
    preview.mutate(next, {
      onSuccess: (result) => {
        setPlan(result);
        setEmpty(result === null || result.plan.steps.length === 0);
      },
    });
  };

  // Plan afresh each time the sheet opens.
  // biome-ignore lint/correctness/useExhaustiveDependencies: once per opening
  useEffect(() => {
    if (!target) return;
    setConfirmed(false);
    review(target);
  }, [target]);

  const runApply = () => {
    if (!target || !plan) return;
    setStage("applying");
    apply.mutate(
      { target, fingerprint: plan.plan.fingerprint, confirmed },
      {
        onSuccess: (result) => {
          setOutcome(result);
          if (result !== null && result.type !== "applied") return;
          toast.success(
            target.on
              ? t("inspector.route.started", { hostname: target.hostname })
              : t("inspector.route.stopped", { hostname: target.hostname }),
          );
          onClose();
          if (target.on) void navigate({ to: "/inspector", search: { host: target.hostname } });
        },
        onError: (error) =>
          toIpcError(error).code === "conflict" ? review(target) : setStage("review"),
      },
    );
  };

  const failure = preview.error ?? apply.error;
  const error = failure ? toIpcError(failure) : null;
  const failed = outcome && outcome.type !== "applied" ? outcome : null;
  const steps = plan?.plan.steps ?? [];
  const title = !target
    ? ""
    : target.on
      ? t("inspector.route.sheetOn", { hostname: target.hostname })
      : restore
        ? t("inspector.route.sheetRestore", { hostname: target.hostname })
        : t("inspector.route.sheetOff", { hostname: target.hostname });
  const applyLabel = target?.on
    ? t("inspector.route.applyOn")
    : restore
      ? t("inspector.route.restore")
      : t("inspector.route.stop");

  const footer =
    stage === "applying" ? (
      failed ? (
        <>
          <Button className="mr-auto" onClick={() => target && review(target)}>
            {t("routeSheet.reviewAgain")}
          </Button>
          <SheetClose asChild>
            <Button variant="primary">{t("common.close")}</Button>
          </SheetClose>
        </>
      ) : (
        <Button pending>{t("routeSheet.applying")}</Button>
      )
    ) : (
      <>
        <SheetClose asChild>
          <Button>{t("common.cancel")}</Button>
        </SheetClose>
        {empty ? null : (
          <Button
            variant="primary"
            disabled={
              !plan ||
              preview.isPending ||
              steps.length === 0 ||
              (plan.plan.requiresConfirmation && !confirmed)
            }
            onClick={runApply}
          >
            {applyLabel}
          </Button>
        )}
      </>
    );

  return (
    <Sheet open={open} onOpenChange={(next) => !next && stage !== "applying" && onClose()}>
      <SheetContent
        title={title}
        description={
          target?.on ? t("inspector.route.descriptionOn") : t("inspector.route.descriptionOff")
        }
        footer={footer}
        onEscapeKeyDown={(event) => stage === "applying" && event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        <div className="flex flex-col gap-3" aria-live="polite" aria-busy={preview.isPending}>
          {stage === "applying" && plan ? (
            <PlanSteps steps={steps} states={apply.steps} />
          ) : preview.isPending ? (
            <div className="flex items-center gap-2 text-body text-secondary">
              <Spinner className="size-3.5" /> {t("routeSheet.reading")}
            </div>
          ) : empty ? (
            <p className="text-body text-secondary">{t("inspector.route.nothing")}</p>
          ) : plan ? (
            <>
              <PlanSteps steps={steps} warnings={plan.plan.warnings} />
              {plan.plan.requiresConfirmation ? (
                <label htmlFor="inspect-confirm" className="flex items-center gap-2 text-body">
                  <Checkbox
                    id="inspect-confirm"
                    checked={confirmed}
                    onCheckedChange={(value) => setConfirmed(value === true)}
                  />
                  {t("routeSheet.confirm.records")}
                </label>
              ) : null}
              {target?.on ? (
                <p className="text-callout text-secondary">{t("inspector.route.lasts")}</p>
              ) : null}
            </>
          ) : null}
          {failed ? (
            <div role="alert" className="flex flex-col gap-1 text-callout">
              <p className="text-error">{translate(failed.error)}</p>
              <p className="text-secondary">
                {failed.type === "rolledBack"
                  ? t("routeSheet.rolledBack")
                  : t("routeSheet.leftovers")}
              </p>
              {failed.type === "partiallyApplied" ? (
                <ul className="list-disc pl-5 text-secondary">
                  {failed.leftovers.map((leftover) => {
                    const text = translate(leftover);
                    return <li key={text}>{text}</li>;
                  })}
                </ul>
              ) : null}
            </div>
          ) : null}
          {error ? (
            <p role="alert" className="text-callout text-error">
              {error.message}
              {error.hint ? <span className="block text-secondary">{error.hint}</span> : null}
            </p>
          ) : null}
        </div>
      </SheetContent>
    </Sheet>
  );
}
