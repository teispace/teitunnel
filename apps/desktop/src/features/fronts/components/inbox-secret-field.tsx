import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { t } from "@/lib/i18n";
import type { InboxVerify } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useSaveInboxSecret } from "../queries";

/**
 * The signing secret a verifying inbox checks webhooks with: "saved" once it's in the
 * keychain (it's never read back), or a field to paste it into.
 */
export function InboxSecretField({
  hostname,
  verify,
  saved,
}: {
  hostname: string;
  verify: InboxVerify;
  saved: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [secret, setSecret] = useState("");
  const save = useSaveInboxSecret(hostname);
  if (saved && !editing) {
    return (
      <div className="flex items-center gap-2 text-callout">
        <span className="text-secondary">{t("fronts.inbox.verify.secretSaved")}</span>
        <Button variant="plain" onClick={() => setEditing(true)}>
          {t("fronts.inbox.verify.secretReplace")}
        </Button>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <Input
          type="password"
          aria-label={t("fronts.inbox.verify.secret")}
          placeholder={t("fronts.inbox.verify.secretPlaceholder")}
          autoComplete="off"
          value={secret}
          onChange={(event) => setSecret(event.target.value)}
          className="min-w-0 flex-1 font-mono text-mono"
        />
        <Button
          disabled={secret.trim() === ""}
          pending={save.isPending}
          onClick={() =>
            save.mutate(
              { verify, secret },
              {
                onSuccess: () => {
                  setSecret("");
                  setEditing(false);
                },
              },
            )
          }
        >
          {t("fronts.inbox.verify.secretSave")}
        </Button>
      </div>
      {save.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(save.error).message}
        </p>
      ) : saved ? null : (
        <p className="text-callout text-secondary">{t("fronts.inbox.verify.secretNeeded")}</p>
      )}
    </div>
  );
}
