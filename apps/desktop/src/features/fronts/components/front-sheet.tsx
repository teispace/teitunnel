import { type FormEvent, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { TextArea } from "@/components/ui/text-area";
import { PermissionFix, type PermissionNeed } from "@/features/accounts";
import { PlanSteps } from "@/features/routes/components/plan-steps";
import { type MessageKey, t, translate } from "@/lib/i18n";
import type {
  FrontChange,
  InboxSettings,
  InboxVerify,
  OfflinePage,
  Outcome,
  PlanView,
} from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import {
  applyFrontDirectly,
  undoChange,
  useFrontApply,
  useFrontPreview,
  useInboxSecrets,
} from "../queries";
import { InboxSecretField } from "./inbox-secret-field";

type Stage = "form" | "review" | "applying";

/** What the sheet edits: a route's offline page, or one of its webhook inboxes. */
export type FrontTarget =
  | { kind: "offline"; hostname: string; current: OfflinePage | null }
  | { kind: "inbox"; hostname: string; path: string | null; current: InboxSettings | null };

const PERMISSION_KEYS = [
  "core.error.observe.workersPermission",
  "core.error.cloudflare.permission",
];

const verifyLabels: Record<InboxVerify | "none", MessageKey> = {
  none: "fronts.inbox.verify.none",
  github: "fronts.inbox.verify.github",
  stripe: "fronts.inbox.verify.stripe",
  standard: "fronts.inbox.verify.standard",
};

export const DEFAULT_PAGE: OfflinePage = {
  title: "Back soon",
  message: "This site runs on a computer that's offline right now. Try again later.",
  whenAppDown: false,
};

const DEFAULT_INBOX: InboxSettings = { maxItems: 500, retentionDays: 7, verify: null };

function zoneOf(hostname: string): string {
  return hostname.split(".").slice(-2).join(".");
}

/**
 * The offline page or a webhook inbox for one route: form → review the plan (Worker,
 * route, database) → apply with live progress, then Undo puts it back as it was.
 */
export function FrontSheet({
  accountId,
  target,
  open,
  onClose,
}: {
  accountId: string;
  target: FrontTarget;
  open: boolean;
  onClose: () => void;
}) {
  const [stage, setStage] = useState<Stage>("form");
  const [page, setPage] = useState<OfflinePage>(DEFAULT_PAGE);
  const [inbox, setInbox] = useState<InboxSettings>(DEFAULT_INBOX);
  const [path, setPath] = useState("/webhooks/");
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [change, setChange] = useState<FrontChange | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const preview = useFrontPreview(accountId);
  const apply = useFrontApply(accountId);
  const exists = target.current !== null;
  const secrets = useInboxSecrets(target.hostname);
  // A verifying inbox needs its sender's signing secret before it can be planned.
  const needsSecret =
    target.kind === "inbox" &&
    inbox.verify !== null &&
    inbox.verify !== undefined &&
    !(secrets.data ?? []).includes(inbox.verify);

  // biome-ignore lint/correctness/useExhaustiveDependencies: runs once per opening
  useEffect(() => {
    if (!open) return;
    setPage(target.kind === "offline" ? (target.current ?? DEFAULT_PAGE) : DEFAULT_PAGE);
    setInbox(target.kind === "inbox" ? (target.current ?? DEFAULT_INBOX) : DEFAULT_INBOX);
    setPath(target.kind === "inbox" ? (target.path ?? "/webhooks/") : "/webhooks/");
    setStage("form");
    setPlan(null);
    setChange(null);
    setOutcome(null);
    preview.reset();
    apply.reset();
  }, [open]);

  const review = (next: FrontChange) => {
    setChange(next);
    preview.mutate(next, {
      onSuccess: (result) => {
        setPlan(result);
        setStage("review");
      },
    });
  };

  const changeFor = (remove: boolean): FrontChange =>
    target.kind === "offline"
      ? { type: "offline", hostname: target.hostname, page: remove ? null : page }
      : {
          type: "inbox",
          hostname: target.hostname,
          path: target.path ?? path,
          inbox: remove ? null : inbox,
        };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    review(changeFor(false));
  };

  const runApply = async () => {
    if (!plan || !change) return;
    setStage("applying");
    const undo = await undoChange(accountId, change).catch(() => null);
    apply.mutate(
      { change, fingerprint: plan.fingerprint },
      {
        onSuccess: (result) => {
          setOutcome(result);
          if (result.type !== "applied") return;
          toast.success(t("fronts.applied", { hostname: target.hostname }), {
            duration: 10_000,
            ...(undo
              ? {
                  action: {
                    label: t("common.undo"),
                    onClick: () =>
                      void applyFrontDirectly(accountId, undo).catch((error: unknown) =>
                        toast.error(t("routeSheet.undoFailed"), {
                          description: toIpcError(error).message,
                        }),
                      ),
                  },
                }
              : {}),
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
  const permission = PERMISSION_KEYS.includes(failure?.key ?? "");
  const needs: PermissionNeed[] = [
    { kind: "workers" },
    { kind: "workersRoutes", zone: zoneOf(target.hostname) },
    ...(target.kind === "inbox" ? [{ kind: "d1" } as const] : []),
  ];
  const failed = outcome && outcome.type !== "applied" ? outcome : null;
  const title =
    target.kind === "offline"
      ? t("fronts.offline.sheetTitle", { hostname: target.hostname })
      : t("fronts.inbox.sheetTitle", { hostname: target.hostname });

  const footer = (() => {
    switch (stage) {
      case "form":
        return (
          <>
            {exists ? (
              <Button
                variant="destructive"
                className="mr-auto"
                pending={preview.isPending && change !== null && isRemoval(change)}
                onClick={() => review(changeFor(true))}
              >
                {t("fronts.remove")}
              </Button>
            ) : null}
            <SheetClose asChild>
              <Button>{t("common.cancel")}</Button>
            </SheetClose>
            <Button
              variant="primary"
              type="submit"
              form="front-form"
              pending={preview.isPending}
              disabled={needsSecret}
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
              onClick={() => void runApply()}
            >
              {change && isRemoval(change) ? t("fronts.applyRemove") : t("fronts.apply")}
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
        title={title}
        description={
          stage === "form"
            ? target.kind === "offline"
              ? t("fronts.offline.description")
              : t("fronts.inbox.description")
            : t("routeSheet.description.review")
        }
        footer={footer}
        onEscapeKeyDown={(event) => stage === "applying" && event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        {stage === "form" ? (
          <form id="front-form" onSubmit={submit} className="flex flex-col gap-5">
            {target.kind === "offline" ? (
              <>
                <Field label={t("fronts.offline.title")}>
                  {(props) => (
                    <Input
                      {...props}
                      maxLength={120}
                      required
                      value={page.title}
                      onChange={(event) => setPage({ ...page, title: event.target.value })}
                    />
                  )}
                </Field>
                <Field label={t("fronts.offline.message")}>
                  {(props) => (
                    <TextArea
                      {...props}
                      rows={3}
                      maxLength={1000}
                      value={page.message}
                      onChange={(event) => setPage({ ...page, message: event.target.value })}
                    />
                  )}
                </Field>
                <label htmlFor="front-app-down" className="flex items-start gap-2 text-body">
                  <Switch
                    id="front-app-down"
                    checked={page.whenAppDown ?? false}
                    onCheckedChange={(whenAppDown) => setPage({ ...page, whenAppDown })}
                  />
                  <span className="flex flex-col">
                    {t("fronts.offline.whenAppDown")}
                    <span className="text-callout text-secondary">
                      {t("fronts.offline.whenAppDownHelp")}
                    </span>
                  </span>
                </label>
              </>
            ) : (
              <>
                <Field label={t("fronts.inbox.path")} help={t("fronts.inbox.pathHelp")}>
                  {(props) => (
                    <Input
                      {...props}
                      className="font-mono"
                      disabled={target.path !== null}
                      value={target.path ?? path}
                      onChange={(event) => setPath(event.target.value.trim())}
                    />
                  )}
                </Field>
                <div className="flex flex-wrap items-center gap-2 text-body">
                  <span>{t("fronts.inbox.keep")}</span>
                  <Input
                    aria-label={t("fronts.inbox.maxItems")}
                    inputMode="numeric"
                    className="w-20 tabular"
                    value={String(inbox.maxItems)}
                    onChange={(event) =>
                      setInbox({
                        ...inbox,
                        maxItems: Number.parseInt(event.target.value.replace(/\D/g, ""), 10) || 0,
                      })
                    }
                  />
                  <span>{t("fronts.inbox.webhooksFor")}</span>
                  <Input
                    aria-label={t("fronts.inbox.retention")}
                    inputMode="numeric"
                    className="w-16 tabular"
                    value={String(inbox.retentionDays)}
                    onChange={(event) =>
                      setInbox({
                        ...inbox,
                        retentionDays:
                          Number.parseInt(event.target.value.replace(/\D/g, ""), 10) || 0,
                      })
                    }
                  />
                  <span>{t("fronts.inbox.days")}</span>
                </div>
                <Field label={t("fronts.inbox.verify.label")} help={t("fronts.inbox.verify.help")}>
                  {() => (
                    <Select
                      label={t("fronts.inbox.verify.label")}
                      options={(["none", "github", "stripe", "standard"] as const).map((value) => ({
                        value,
                        label: t(verifyLabels[value]),
                      }))}
                      value={inbox.verify ?? "none"}
                      onValueChange={(value) =>
                        setInbox({
                          ...inbox,
                          verify: value === "none" ? null : (value as InboxVerify),
                        })
                      }
                    />
                  )}
                </Field>
                {inbox.verify ? (
                  <InboxSecretField
                    hostname={target.hostname}
                    verify={inbox.verify}
                    saved={!needsSecret}
                  />
                ) : null}
              </>
            )}
            <p className="text-callout text-secondary">{t("fronts.cost")}</p>
            {permission ? (
              <PermissionFix
                accountId={accountId}
                needs={needs}
                refused
                onReady={() => change && review(change)}
              />
            ) : failure ? (
              <p role="alert" className="text-callout text-error">
                {failure.message}
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
            ) : failure ? null : (
              <div className="flex items-center gap-2 text-body text-secondary">
                <Spinner className="size-3.5" /> {t("routeSheet.reading")}
              </div>
            )}
            {failure ? (
              <p role="alert" className="text-callout text-error">
                {failure.message}
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

function isRemoval(change: FrontChange): boolean {
  return change.type === "offline" ? change.page === null : change.inbox === null;
}
