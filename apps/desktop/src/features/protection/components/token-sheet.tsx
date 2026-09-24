import { KeyRound } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { PlanSteps } from "@/features/routes/components/plan-steps";
import { t, translate } from "@/lib/i18n";
import type { IssuedTokenView, PlanView, ProtectionChange } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { copySecret, forgetSecret, useProtectionApply, useProtectionPreview } from "../queries";

type Stage = "form" | "review" | "applying" | "issued";

interface TokenSheetProps {
  accountId: string;
  hostname: string;
  open: boolean;
  /** A token rotated elsewhere: show its new secret straight away. */
  issued?: IssuedTokenView | null;
  onClose: () => void;
}

/** A dotted stand-in: the secret itself never reaches the webview. */
const MASK = "••••••••••••••••••••••••";

/**
 * Creates a service token for machines (name → review → create), then shows its
 * credentials once: the client id, and the secret, which is copied to the clipboard
 * from Rust without ever reaching this window.
 */
export function TokenSheet({ accountId, hostname, open, issued = null, onClose }: TokenSheetProps) {
  const [stage, setStage] = useState<Stage>("form");
  const [name, setName] = useState("CI");
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [change, setChange] = useState<ProtectionChange | null>(null);
  const [token, setToken] = useState<IssuedTokenView | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const preview = useProtectionPreview(accountId);
  const apply = useProtectionApply(accountId);

  // biome-ignore lint/correctness/useExhaustiveDependencies: runs once per opening
  useEffect(() => {
    if (!open) return;
    setStage(issued ? "issued" : "form");
    setToken(issued);
    setName("CI");
    setPlan(null);
    setFailure(null);
    preview.reset();
    apply.reset();
  }, [open]);

  const close = () => {
    if (token) void forgetSecret(token.tokenId);
    onClose();
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const next: ProtectionChange = { type: "createToken", hostname, label: name };
    setChange(next);
    preview.mutate(next, {
      onSuccess: (result) => {
        setPlan(result);
        setStage("review");
      },
    });
  };

  const create = () => {
    if (!plan || !change) return;
    setStage("applying");
    apply.mutate(
      { change, fingerprint: plan.fingerprint },
      {
        onSuccess: ({ outcome, issued: tokens }) => {
          if (outcome.type !== "applied") {
            setFailure(translate(outcome.error));
            setStage("review");
            return;
          }
          setToken(tokens[0] ?? null);
          setStage("issued");
        },
        onError: () => setStage("review"),
      },
    );
  };

  const error = preview.error ?? apply.error;
  const message = failure ?? (error ? toIpcError(error).message : null);

  const footer =
    stage === "form" ? (
      <>
        <SheetClose asChild>
          <Button>{t("common.cancel")}</Button>
        </SheetClose>
        <Button
          variant="primary"
          type="submit"
          form="token-form"
          disabled={name.trim() === ""}
          pending={preview.isPending}
        >
          {preview.isPending ? t("routeSheet.checking") : t("routeSheet.review")}
        </Button>
      </>
    ) : stage === "review" || stage === "applying" ? (
      <>
        <Button
          className="mr-auto"
          disabled={stage === "applying"}
          onClick={() => setStage("form")}
        >
          {t("routeSheet.back")}
        </Button>
        <SheetClose asChild>
          <Button disabled={stage === "applying"}>{t("common.cancel")}</Button>
        </SheetClose>
        <Button
          variant="primary"
          disabled={!plan || plan.steps.length === 0}
          pending={stage === "applying"}
          onClick={create}
        >
          {t("serviceTokens.create")}
        </Button>
      </>
    ) : (
      <Button variant="primary" onClick={close}>
        {t("common.done")}
      </Button>
    );

  return (
    <Sheet open={open} onOpenChange={(next) => !next && stage !== "applying" && close()}>
      <SheetContent
        title={stage === "issued" ? t("serviceTokens.issuedTitle") : t("serviceTokens.sheetTitle")}
        description={
          stage === "issued"
            ? t("serviceTokens.issuedDescription", { hostname })
            : t("serviceTokens.description", { hostname })
        }
        footer={footer}
        onEscapeKeyDown={(event) => stage === "applying" && event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        {stage === "form" ? (
          <form id="token-form" onSubmit={submit} className="flex flex-col gap-4">
            <Field label={t("serviceTokens.name.label")} help={t("serviceTokens.name.help")}>
              {(control) => (
                <Input
                  {...control}
                  autoFocus
                  maxLength={40}
                  autoComplete="off"
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                />
              )}
            </Field>
            {message ? (
              <p role="alert" className="text-callout text-error">
                {message}
              </p>
            ) : null}
          </form>
        ) : null}
        {stage === "review" || stage === "applying" ? (
          <div className="flex flex-col gap-3" aria-busy={stage === "applying"}>
            {plan ? (
              <PlanSteps
                steps={plan.steps}
                warnings={plan.warnings}
                {...(stage === "applying" ? { states: apply.steps } : {})}
              />
            ) : (
              <div className="flex items-center gap-2 text-body text-secondary">
                <Spinner className="size-3.5" /> {t("routeSheet.reading")}
              </div>
            )}
            {message ? (
              <p role="alert" className="text-callout text-error">
                {message}
              </p>
            ) : null}
          </div>
        ) : null}
        {stage === "issued" && token ? (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-2 text-headline">
              <KeyRound aria-hidden className="size-4 text-healthy" strokeWidth={1.75} />
              {token.name}
            </div>
            <Field label={t("serviceTokens.clientId")}>
              {() => <CopyField label={t("serviceTokens.clientId")} value={token.clientId} />}
            </Field>
            <Field label={t("serviceTokens.clientSecret")} help={t("serviceTokens.secretOnce")}>
              {() => (
                <CopyField
                  label={t("serviceTokens.clientSecret")}
                  value={MASK}
                  onCopy={() => copySecret(token.tokenId, "secret")}
                />
              )}
            </Field>
            <div>
              <Button size="sm" onClick={() => void copySecret(token.tokenId, "headers")}>
                {t("serviceTokens.copyHeaders")}
              </Button>
            </div>
          </div>
        ) : null}
      </SheetContent>
    </Sheet>
  );
}
