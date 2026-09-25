import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { t } from "@/lib/i18n";
import type { WebhookCheck } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { providerNames } from "../model";
import { useSetWebhookSecret } from "../queries";

/** The signature check's badge: valid, invalid, expired, or why it couldn't be checked. */
export function WebhookBadge({ check }: { check: WebhookCheck }) {
  const verdict = check.verification;
  if (!verdict) return <Badge>{t("inspector.webhook.unchecked")}</Badge>;
  switch (verdict.result) {
    case "valid":
      return <Badge tone="healthy">{t("inspector.webhook.valid")}</Badge>;
    case "invalid":
      return <Badge tone="error">{t("inspector.webhook.invalid")}</Badge>;
    case "expired":
      return <Badge tone="warning">{t("inspector.webhook.expired")}</Badge>;
    case "unknownProvider":
      return <Badge>{t("inspector.webhook.unknown")}</Badge>;
    case "notEnoughData":
      return <Badge>{t("inspector.webhook.notEnoughData")}</Badge>;
  }
}

/**
 * A recognised webhook: who sent it, whether its signature holds, and the signing secret
 * for this share or route. The secret goes to the keychain and never comes back.
 */
export function WebhookPanel({ check, tap }: { check: WebhookCheck; tap: string }) {
  const [adding, setAdding] = useState(false);
  const [secret, setSecret] = useState("");
  const save = useSetWebhookSecret();
  const provider = providerNames[check.provider];
  const verdict = check.verification;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!secret.trim()) return;
    save.mutate(
      { tap, provider: check.provider, secret: secret.trim() },
      {
        onSuccess: () => {
          setSecret("");
          setAdding(false);
        },
      },
    );
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2 text-body">
        <span>{t("inspector.webhook.from", { provider })}</span>
        <WebhookBadge check={check} />
      </div>
      {verdict?.result === "invalid" || verdict?.result === "notEnoughData" ? (
        <p className="selectable text-callout text-secondary">{verdict.reason}</p>
      ) : null}
      {verdict?.result === "expired" && verdict.ageSecs !== null ? (
        <p className="text-callout text-secondary">
          {t("inspector.webhook.age", { minutes: Math.round(verdict.ageSecs / 60) })}
        </p>
      ) : null}
      {check.hasSecret ? (
        <div className="flex items-center gap-2">
          <p className="min-w-0 flex-1 text-callout text-secondary">
            {t("inspector.webhook.saved")}
          </p>
          <Button
            size="sm"
            variant="destructive"
            pending={save.isPending}
            onClick={() => save.mutate({ tap, provider: check.provider, secret: null })}
          >
            {t("inspector.webhook.remove")}
          </Button>
        </div>
      ) : adding ? (
        <form onSubmit={submit} className="flex items-center gap-2">
          <Input
            type="password"
            autoComplete="off"
            aria-label={t("inspector.webhook.secretLabel", { provider })}
            placeholder={t("inspector.webhook.secretLabel", { provider })}
            value={secret}
            onChange={(event) => setSecret(event.target.value)}
            className="min-w-0 flex-1 font-mono text-mono"
            autoFocus
          />
          <Button
            size="sm"
            onClick={() => {
              setAdding(false);
              setSecret("");
            }}
          >
            {t("common.cancel")}
          </Button>
          <Button
            size="sm"
            variant="primary"
            type="submit"
            disabled={!secret.trim()}
            pending={save.isPending}
          >
            {t("inspector.webhook.save")}
          </Button>
        </form>
      ) : (
        <div className="flex items-center gap-2">
          <p className="min-w-0 flex-1 text-callout text-secondary">
            {t("inspector.webhook.noSecret")}
          </p>
          <Button size="sm" onClick={() => setAdding(true)}>
            {t("inspector.webhook.add")}
          </Button>
        </div>
      )}
      {save.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(save.error).message}
        </p>
      ) : null}
    </div>
  );
}
