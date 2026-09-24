import { type ReactNode, useId, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import { ExposureNotice } from "@/features/exposure";
import { PlanSteps } from "@/features/routes";
import { t, translate } from "@/lib/i18n";
import type { ProjectPlan, SnapshotSourceDecl } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useApplyProject } from "../queries";

function sourceText(source: SnapshotSourceDecl): string {
  return source.path;
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-headline">{title}</h3>
      {children}
    </section>
  );
}

interface ApplySheetProps {
  plan: ProjectPlan | null;
  onClose: () => void;
}

/**
 * The combined plan of a project: every route change (the engine's steps), the Snapshots
 * it publishes and the shares it starts. Nothing happens until Apply.
 */
export function ApplySheet({ plan, onClose }: ApplySheetProps) {
  const apply = useApplyProject();
  const [confirmed, setConfirmed] = useState(false);
  const confirmId = useId();
  const error = apply.error ? toIpcError(apply.error) : null;
  const failure = apply.data?.routes.failure ?? null;
  const close = () => {
    apply.reset();
    setConfirmed(false);
    onClose();
  };
  const submit = () => {
    if (!plan) return;
    apply.mutate(
      { path: plan.path, fingerprint: plan.fingerprint, confirmed },
      {
        onSuccess: (result) => {
          if (result.routes.failure) return;
          const started = result.shares.length;
          toast.success(t("project.applied", { name: plan.name }), {
            description: started > 0 ? t("project.sharesStarted", { count: started }) : undefined,
          });
          close();
        },
      },
    );
  };

  return (
    <Sheet open={plan !== null} onOpenChange={(open) => !open && close()}>
      {plan ? (
        <SheetContent
          width="lg"
          title={t("project.sheet.title", { name: plan.name })}
          description={t("project.sheet.description")}
          footer={
            <>
              <Button onClick={close}>{t("common.cancel")}</Button>
              <Button
                variant="primary"
                pending={apply.isPending}
                disabled={plan.requiresConfirmation && !confirmed}
                onClick={submit}
              >
                {t("project.sheet.apply")}
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-4">
            {plan.routes.length > 0 ? (
              <Section title={t("project.sheet.routes")}>
                {plan.routes.map((route) => (
                  <div
                    key={`${route.hostname} ${route.path ?? ""}`}
                    className="flex flex-col gap-1.5"
                  >
                    <p className="font-mono text-mono text-secondary">
                      {route.hostname}
                      {route.path ? ` ${route.path}` : ""}
                    </p>
                    <PlanSteps steps={route.plan.steps} warnings={route.plan.warnings} />
                    {route.change.type === "addRoute" ? (
                      <ExposureNotice origin={route.change.route.origin} />
                    ) : null}
                  </div>
                ))}
              </Section>
            ) : null}
            {plan.snapshots.length > 0 ? (
              <Section title={t("project.sheet.snapshots")}>
                <ul className="flex flex-col gap-1 rounded-card bg-surface-inset px-3 py-2 text-body">
                  {plan.snapshots.map((snapshot) => (
                    <li key={snapshot.name}>
                      {snapshot.exists
                        ? t("project.sheet.snapshotUpdate", { name: snapshot.name })
                        : t("project.sheet.snapshotNew", {
                            name: snapshot.name,
                            source: sourceText(snapshot.source),
                            address: snapshot.hostname ?? t("project.sheet.workersDev"),
                          })}
                    </li>
                  ))}
                </ul>
              </Section>
            ) : null}
            {plan.shares.length > 0 ? (
              <Section title={t("project.sheet.shares")}>
                <ul className="flex flex-col gap-1 rounded-card bg-surface-inset px-3 py-2 text-body">
                  {plan.shares.map((share) => (
                    <li key={`${share.origin} ${share.hostname ?? ""}`}>
                      {share.hostname
                        ? t("project.sheet.shareDomain", {
                            origin: share.origin,
                            hostname: share.hostname,
                          })
                        : t("project.sheet.shareQuick", { origin: share.origin })}
                    </li>
                  ))}
                </ul>
                <p className="text-footnote text-secondary">{t("project.sheet.sharesNote")}</p>
              </Section>
            ) : null}
            {plan.requiresConfirmation ? (
              <label htmlFor={confirmId} className="flex items-center gap-2 text-body">
                <Checkbox
                  id={confirmId}
                  checked={confirmed}
                  onCheckedChange={(value) => setConfirmed(value === true)}
                />
                {t("project.sheet.confirm")}
              </label>
            ) : null}
            {failure ? (
              <p role="alert" className="text-callout text-error">
                {t("project.failed", { hostname: failure.hostname })}{" "}
                <span className="text-secondary">{translate(failure.error)}</span>
              </p>
            ) : null}
            {error ? (
              <p role="alert" className="text-callout text-error">
                {error.message}
                {error.hint ? <span className="block text-secondary">{error.hint}</span> : null}
              </p>
            ) : null}
          </div>
        </SheetContent>
      ) : null}
    </Sheet>
  );
}
