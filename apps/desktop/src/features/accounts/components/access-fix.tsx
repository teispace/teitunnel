import { ExternalLink, KeyRound } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { openTokenPage, useAccounts, useAddToken, useCapabilities } from "../queries";

/**
 * Replaces the account's credential with a new token that has every permission. Not a
 * `<form>`: it's shown inside the route form, and forms can't nest.
 */
function NewToken({ detail, onConnected }: { detail: string; onConnected: () => void }) {
  const [token, setToken] = useState("");
  const addToken = useAddToken();
  const canSubmit = token.trim() !== "" && !addToken.isPending;
  const submit = () => {
    if (!canSubmit) return;
    addToken.mutate(token, {
      onSuccess: () => {
        setToken("");
        onConnected();
      },
    });
  };
  return (
    <div className="flex flex-col gap-2">
      <p className="text-callout text-secondary">{detail}</p>
      <div>
        <Button size="sm" onClick={() => void openTokenPage("create")}>
          {t("accessFix.createToken")} <ExternalLink />
        </Button>
      </div>
      <Field
        label={t("connect.token")}
        error={addToken.error ? toIpcError(addToken.error).message : null}
      >
        {(control) => (
          <Input
            {...control}
            type="password"
            autoComplete="off"
            placeholder={t("connect.tokenPlaceholder")}
            value={token}
            onChange={(event) => {
              setToken(event.target.value);
              if (addToken.error) addToken.reset();
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter") return;
              // Keep Enter from submitting the form around this one.
              event.preventDefault();
              submit();
            }}
          />
        )}
      </Field>
      <div>
        <Button size="sm" variant="primary" disabled={!canSubmit} onClick={submit}>
          {addToken.isPending ? t("connect.checking") : t("connect.connect")}
        </Button>
      </div>
    </div>
  );
}

interface AccessFixProps {
  accountId: string;
  /** Cloudflare already refused: show the fix even if an older check said allowed. */
  refused?: boolean;
  /** Called when a re-check finds the permissions, e.g. to review the change again. */
  onReady?: () => void;
}

/**
 * Shown where a login is needed but the account's credential can't manage Access. It
 * explains how to add the permissions (the token itself stays the same) and checks
 * again whenever the window regains focus, so coming back from the browser is enough.
 */
export function AccessFix({ accountId, refused = false, onReady }: AccessFixProps) {
  const account = useAccounts().data?.find((a) => a.id === accountId);
  const caps = useCapabilities(accountId);
  const [checked, setChecked] = useState(false);
  const [newToken, setNewToken] = useState(false);
  const grant = caps.data?.accessEdit;
  const { refetch } = caps;

  // Only an explicit re-check (focus, Check Again, a new token) reports readiness, so a
  // refusal that persists can't loop through onReady.
  const recheck = useCallback(async () => {
    const { data } = await refetch();
    setChecked(true);
    if (data?.accessEdit !== "yes") return;
    toast.success(t("accessFix.ready"));
    onReady?.();
  }, [refetch, onReady]);

  useEffect(() => {
    const onFocus = () => void recheck();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [recheck]);

  // Wait for both, so the right path shows from the first frame.
  if (!account || caps.isPending || (grant === "yes" && !refused)) return null;
  const token = account?.credential === "apiToken";

  return (
    <section
      aria-label={t("accessFix.title")}
      className="flex gap-3 rounded-card bg-surface-inset p-4"
    >
      <KeyRound aria-hidden className="mt-0.5 size-4 shrink-0 text-warning" />
      <div className="flex min-w-0 flex-1 flex-col gap-3">
        <h3 className="text-headline">{t("accessFix.title")}</h3>
        {token && !newToken ? (
          <>
            <p className="text-callout text-secondary">{t("accessFix.editDetail")}</p>
            <ol className="flex list-decimal flex-col gap-1 pl-5 text-callout">
              <li>{t("accessFix.step1")}</li>
              <li>{t("accessFix.step2")}</li>
              <li>{t("accessFix.step3")}</li>
            </ol>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" variant="primary" onClick={() => void openTokenPage("edit")}>
                {t("accessFix.openTokens")} <ExternalLink />
              </Button>
              <Button size="sm" disabled={caps.isFetching} onClick={() => void recheck()}>
                {caps.isFetching ? (
                  <>
                    <Spinner className="size-3" /> {t("accessFix.checking")}
                  </>
                ) : (
                  t("accessFix.checkAgain")
                )}
              </Button>
            </div>
            <p aria-live="polite" className="text-footnote text-secondary">
              {checked && !caps.isFetching && grant !== "yes"
                ? t("accessFix.stillMissing")
                : t("accessFix.autoCheck")}
            </p>
            <div className="-mt-1">
              <Button
                size="sm"
                variant="plain"
                className="-ml-2.5"
                onClick={() => setNewToken(true)}
              >
                {t("accessFix.useNewToken")}
              </Button>
            </div>
          </>
        ) : (
          <NewToken
            detail={token ? t("accessFix.newTokenDetail") : t("accessFix.otherDetail")}
            onConnected={() => void recheck()}
          />
        )}
      </div>
    </section>
  );
}
