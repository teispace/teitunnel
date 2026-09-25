import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import { HostnameInput, joinHostname, PlanSteps } from "@/features/routes";
import { useApply, usePreview } from "@/features/routes/queries";
import { t, translate } from "@/lib/i18n";
import type { Change, PlanView, ZoneRef } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { checkable } from "./format";
import { HostnameAvailability } from "./hostname-availability";

interface ReserveSheetProps {
  accountId: string;
  zones: readonly ZoneRef[];
  open: boolean;
  onClose: () => void;
}

/** Reserve a hostname (optionally until a date): a form, then the plan to review. */
export function ReserveSheet({ accountId, zones, open, onClose }: ReserveSheetProps) {
  const [hostname, setHostname] = useState(() => joinHostname("", zones[0]?.name ?? ""));
  const [until, setUntil] = useState("");
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const preview = usePreview(accountId);
  const apply = useApply(accountId);
  const change: Change = {
    type: "reserveHostname",
    hostname: hostname.trim().toLowerCase(),
    until: until || null,
  };

  const close = () => {
    setPlan(null);
    setError(null);
    onClose();
  };
  const review = () => {
    setError(null);
    preview.mutate(
      { change, tunnelId: null },
      {
        onSuccess: setPlan,
        onError: (err) => setError(toIpcError(err).message),
      },
    );
  };
  const confirm = () => {
    if (!plan) return;
    apply.mutate(
      {
        change,
        tunnelId: null,
        fingerprint: plan.fingerprint,
        confirmed: plan.requiresConfirmation,
      },
      {
        onSuccess: (outcome) => {
          if (outcome.type === "applied") {
            toast.success(t("reservations.reserved", { hostname: change.hostname }));
            close();
          } else {
            setError(translate(outcome.error));
          }
        },
        onError: (err) => setError(toIpcError(err).message),
      },
    );
  };

  const footer = plan ? (
    <>
      <Button variant="plain" onClick={() => setPlan(null)} disabled={apply.isPending}>
        {t("routeSheet.back")}
      </Button>
      <Button
        variant="primary"
        onClick={confirm}
        disabled={apply.isPending || plan.steps.length === 0}
      >
        {apply.isPending
          ? t("routeSheet.applying")
          : plan.requiresConfirmation
            ? t("reservations.takeOver")
            : t("reservations.reserve")}
      </Button>
    </>
  ) : (
    <>
      <Button variant="plain" onClick={close}>
        {t("common.cancel")}
      </Button>
      <Button
        variant="primary"
        onClick={review}
        disabled={!checkable(change.hostname) || preview.isPending}
      >
        {preview.isPending ? t("routeSheet.checking") : t("routeSheet.review")}
      </Button>
    </>
  );

  return (
    <Sheet open={open} onOpenChange={(next) => (next ? undefined : close())}>
      <SheetContent
        title={t("reservations.reserveTitle")}
        description={plan ? t("routeSheet.description.review") : t("reservations.reserveDetail")}
        footer={footer}
      >
        {plan ? (
          <PlanSteps steps={plan.steps} warnings={plan.warnings} states={apply.steps} />
        ) : (
          <div className="flex flex-col gap-3">
            <Field label={t("reservations.hostname")}>
              {(control) => (
                <HostnameInput
                  id={control.id}
                  zones={zones}
                  value={hostname}
                  onChange={setHostname}
                  describedBy={control["aria-describedby"]}
                  autoFocus
                />
              )}
            </Field>
            <HostnameAvailability accountId={accountId} hostname={change.hostname} />
            <Field label={t("reservations.untilLabel")} help={t("reservations.untilHint")}>
              {(control) => (
                <Input
                  {...control}
                  type="date"
                  value={until}
                  onChange={(event) => setUntil(event.target.value)}
                  className="w-44"
                />
              )}
            </Field>
          </div>
        )}
        {error ? (
          <p role="alert" className="mt-3 text-callout text-error">
            {error}
          </p>
        ) : null}
      </SheetContent>
    </Sheet>
  );
}
