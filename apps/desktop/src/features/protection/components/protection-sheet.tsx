import { type FormEvent, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Select } from "@/components/ui/select";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { PermissionFix } from "@/features/accounts";
import { PlanSteps } from "@/features/routes/components/plan-steps";
import { t, translate } from "@/lib/i18n";
import type { Outcome, PlanView, ProtectionChange, ProtectionView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import {
  actionLabels,
  botLabels,
  complete,
  isOff,
  type Protection,
  periodLabel,
  periods,
} from "../model";
import { applyProtectionDirectly, useProtectionApply, useProtectionPreview } from "../queries";
import { HeaderRules } from "./header-rules";
import { QuotaList } from "./quota-list";

type Stage = "form" | "review" | "applying";

const EDGE_PERMISSION = "core.error.observe.edgePermission";

interface ProtectionSheetProps {
  accountId: string;
  hostname: string;
  /** What the hostname has now (the form starts from it). */
  current: ProtectionView | undefined;
  open: boolean;
  onClose: () => void;
}

/**
 * Edge protection for one hostname: form → review the plan (rules added, changed or
 * removed, quota use) → apply with live progress, then Undo puts the old settings back.
 */
export function ProtectionSheet({
  accountId,
  hostname,
  current,
  open,
  onClose,
}: ProtectionSheetProps) {
  const [stage, setStage] = useState<Stage>("form");
  const [form, setForm] = useState<Protection>(complete(current?.protection));
  const [requests, setRequests] = useState("30");
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [change, setChange] = useState<ProtectionChange | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const preview = useProtectionPreview(accountId);
  const apply = useProtectionApply(accountId);
  const before = complete(current?.protection);
  const rateLimitAvailable = current?.rateLimitAvailable ?? false;
  const longest = current?.longestPeriod ?? 10;

  // Start from what the hostname has whenever the sheet opens.
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs once per opening
  useEffect(() => {
    if (!open) return;
    const start = complete(current?.protection);
    setForm(start);
    setRequests(String(start.rateLimit?.requests ?? 30));
    setStage("form");
    setPlan(null);
    setChange(null);
    setOutcome(null);
    preview.reset();
    apply.reset();
  }, [open]);

  const review = (next: ProtectionChange) => {
    setChange(next);
    preview.mutate(next, {
      onSuccess: (result) => {
        setPlan(result);
        setStage("review");
      },
    });
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const rateLimit = form.rateLimit
      ? { ...form.rateLimit, requests: Number.parseInt(requests, 10) || 0 }
      : null;
    review({ type: "protect", hostname, protection: { ...form, rateLimit } });
  };

  const runApply = () => {
    if (!plan || !change) return;
    setStage("applying");
    apply.mutate(
      { change, fingerprint: plan.fingerprint },
      {
        onSuccess: ({ outcome: result }) => {
          setOutcome(result);
          if (result.type !== "applied") return;
          const undo: ProtectionChange = { type: "protect", hostname, protection: before };
          toast.success(t("protection.applied", { hostname }), {
            duration: 10_000,
            action: {
              label: t("common.undo"),
              onClick: () =>
                void applyProtectionDirectly(accountId, undo).catch((error: unknown) =>
                  toast.error(t("routeSheet.undoFailed"), {
                    description: toIpcError(error).message,
                  }),
                ),
            },
          });
          onClose();
        },
        onError: (error) =>
          toIpcError(error).code === "conflict" ? review(change) : setStage("review"),
      },
    );
  };

  const failure = preview.error
    ? toIpcError(preview.error)
    : apply.error
      ? toIpcError(apply.error)
      : null;
  const fieldError = failure?.field === "protection" ? failure.message : null;
  const generalError = failure && failure.field !== "protection" ? failure : null;
  const permission =
    failure?.key === EDGE_PERMISSION || failure?.key === "core.error.cloudflare.permission";
  const failed = outcome && outcome.type !== "applied" ? outcome : null;

  const footer = (() => {
    switch (stage) {
      case "form":
        return (
          <>
            <SheetClose asChild>
              <Button>{t("common.cancel")}</Button>
            </SheetClose>
            <Button
              variant="primary"
              type="submit"
              form="protection-form"
              pending={preview.isPending}
            >
              {preview.isPending ? t("routeSheet.checking") : t("routeSheet.review")}
            </Button>
          </>
        );
      case "review":
        return (
          <>
            <Button className="mr-auto" onClick={() => setStage("form")}>
              {t("routeSheet.back")}
            </Button>
            <SheetClose asChild>
              <Button>{t("common.cancel")}</Button>
            </SheetClose>
            <Button
              variant="primary"
              disabled={!plan || plan.steps.length === 0 || preview.isPending}
              onClick={runApply}
            >
              {isOff(form) ? t("protection.applyOff") : t("protection.apply")}
            </Button>
          </>
        );
      case "applying":
        return failed ? (
          <>
            <Button className="mr-auto" onClick={() => change && review(change)}>
              {t("routeSheet.reviewAgain")}
            </Button>
            <SheetClose asChild>
              <Button variant="primary">{t("common.close")}</Button>
            </SheetClose>
          </>
        ) : (
          <Button pending>{t("routeSheet.applying")}</Button>
        );
    }
  })();

  return (
    <Sheet open={open} onOpenChange={(next) => !next && stage !== "applying" && onClose()}>
      <SheetContent
        title={t("protection.sheetTitle", { hostname })}
        description={
          stage === "form" ? t("protection.description") : t("routeSheet.description.review")
        }
        footer={footer}
        onEscapeKeyDown={(event) => stage === "applying" && event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        {stage === "form" ? (
          <form id="protection-form" onSubmit={submit} className="flex flex-col gap-5">
            <Field label={t("protection.bots.label")} help={t("protection.bots.help")}>
              {() => (
                <SegmentedControl
                  label={t("protection.bots.label")}
                  segments={(["off", "challenge", "block"] as const).map((value) => ({
                    value,
                    label: t(botLabels[value]),
                  }))}
                  value={form.bots}
                  onValueChange={(bots) => setForm({ ...form, bots })}
                />
              )}
            </Field>
            <label htmlFor="protection-ai" className="flex items-start gap-2 text-body">
              <Switch
                id="protection-ai"
                checked={form.aiCrawlers}
                onCheckedChange={(aiCrawlers) => setForm({ ...form, aiCrawlers })}
              />
              <span className="flex flex-col">
                {t("protection.ai.label")}
                <span className="text-callout text-secondary">{t("protection.ai.help")}</span>
              </span>
            </label>
            <fieldset className="flex flex-col gap-2">
              <label htmlFor="protection-limit" className="flex items-center gap-2 text-body">
                <Checkbox
                  id="protection-limit"
                  disabled={!rateLimitAvailable && form.rateLimit === null}
                  checked={form.rateLimit !== null}
                  onCheckedChange={(on) =>
                    setForm({
                      ...form,
                      rateLimit:
                        on === true
                          ? {
                              requests: Number.parseInt(requests, 10) || 30,
                              period: 60,
                              action: "block",
                            }
                          : null,
                    })
                  }
                />
                {t("protection.rateLimit.toggle")}
              </label>
              {rateLimitAvailable ? null : (
                <p className="text-callout text-secondary">{t("protection.rateLimit.free")}</p>
              )}
              {form.rateLimit ? (
                <div className="flex flex-wrap items-center gap-2 pl-6 text-body">
                  <Input
                    aria-label={t("protection.rateLimit.requests")}
                    inputMode="numeric"
                    className="w-20 tabular"
                    value={requests}
                    onChange={(event) => setRequests(event.target.value.replace(/\D/g, ""))}
                  />
                  <span className="text-secondary">{t("protection.rateLimit.per")}</span>
                  <Select
                    label={t("protection.rateLimit.periodLabel")}
                    options={periods
                      .filter((seconds) => seconds <= longest)
                      .map((seconds) => ({ value: String(seconds), label: periodLabel(seconds) }))}
                    value={String(form.rateLimit.period)}
                    onValueChange={(value) =>
                      form.rateLimit &&
                      setForm({
                        ...form,
                        rateLimit: { ...form.rateLimit, period: Number(value) },
                      })
                    }
                    className="w-28"
                  />
                  <SegmentedControl
                    size="sm"
                    label={t("protection.rateLimit.actionLabel")}
                    segments={(["block", "challenge"] as const).map((value) => ({
                      value,
                      label: t(actionLabels[value]),
                    }))}
                    value={form.rateLimit.action}
                    onValueChange={(action) =>
                      form.rateLimit &&
                      setForm({ ...form, rateLimit: { ...form.rateLimit, action } })
                    }
                  />
                </div>
              ) : null}
              {current && current.sharesRateLimitWith.length > 0 ? (
                <p className="text-callout text-secondary">
                  {t("protection.rateLimit.shared", {
                    hostnames: current.sharesRateLimitWith.join(", "),
                  })}
                </p>
              ) : null}
            </fieldset>
            <HeaderRules
              kind="request"
              value={form.requestHeaders}
              onChange={(requestHeaders) => setForm({ ...form, requestHeaders })}
            />
            <HeaderRules
              kind="response"
              value={form.responseHeaders}
              onChange={(responseHeaders) => setForm({ ...form, responseHeaders })}
            />
            {current ? <QuotaList quotas={current.quotas} zone={current.zone} /> : null}
            {fieldError ? (
              <p role="alert" className="text-callout text-error">
                {fieldError}
              </p>
            ) : null}
            {permission ? (
              <PermissionFix
                accountId={accountId}
                needs={[{ kind: "edgeRules" }]}
                refused
                onReady={() => change && review(change)}
              />
            ) : generalError ? (
              <p role="alert" className="text-callout text-error">
                {generalError.message}
              </p>
            ) : null}
          </form>
        ) : null}

        {stage === "review" ? (
          <div className="flex flex-col gap-3" aria-busy={preview.isPending}>
            {plan && !preview.isPending ? (
              plan.steps.length === 0 ? (
                <p className="text-body text-secondary">{t("routeSheet.nothingToChange")}</p>
              ) : (
                <PlanSteps steps={plan.steps} warnings={plan.warnings} />
              )
            ) : generalError ? null : (
              <div className="flex items-center gap-2 text-body text-secondary">
                <Spinner className="size-3.5" /> {t("routeSheet.reading")}
              </div>
            )}
            {generalError ? (
              <p role="alert" className="text-callout text-error">
                {generalError.message}
              </p>
            ) : null}
          </div>
        ) : null}

        {stage === "applying" && plan ? (
          <div className="flex flex-col gap-3" aria-live="polite">
            <PlanSteps steps={plan.steps} states={apply.steps} />
            {failed ? (
              <div role="alert" className="flex flex-col gap-1 text-callout">
                <p className="text-error">{translate(failed.error)}</p>
                {failed.type === "rolledBack" ? (
                  <p className="text-secondary">{t("routeSheet.rolledBack")}</p>
                ) : (
                  <>
                    <p className="text-secondary">{t("routeSheet.leftovers")}</p>
                    <ul className="list-disc pl-5 text-secondary">
                      {failed.leftovers.map((leftover) => {
                        const text = translate(leftover);
                        return <li key={text}>{text}</li>;
                      })}
                    </ul>
                  </>
                )}
              </div>
            ) : null}
          </div>
        ) : null}
      </SheetContent>
    </Sheet>
  );
}
