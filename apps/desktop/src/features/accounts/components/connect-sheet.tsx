import { ExternalLink } from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { toast } from "sonner";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Sheet, SheetClose, SheetContent, SheetTrigger } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import {
  openTokenPage,
  useAddToken,
  useCertDetected,
  useImportCert,
  useOAuthAvailable,
  useOAuthSignIn,
} from "../queries";

function Step({ n, title, children }: { n: number; title: string; children: ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-surface-control text-footnote font-semibold tabular">
        {n}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <h3 className="text-headline">{title}</h3>
        {children}
      </div>
    </li>
  );
}

/** Connect a Cloudflare account with an API token (or an existing cloudflared login). */
export function ConnectSheet({ trigger }: { trigger: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [token, setToken] = useState("");
  const addToken = useAddToken();
  const importCert = useImportCert();
  const cert = useCertDetected(open);
  const oauthAvailable = useOAuthAvailable().data === true;
  const signIn = useOAuthSignIn();
  const error = addToken.error ?? importCert.error;
  const message = error ? toIpcError(error).message : null;

  const finish = (names: string[]) => {
    setOpen(false);
    setToken("");
    toast.success(
      names.length === 1
        ? t("connect.connectedOne", { name: names[0] ?? "" })
        : t("connect.connectedMany", { count: names.length }),
    );
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    addToken.mutate(token, { onSuccess: (accounts) => finish(accounts.map((a) => a.name)) });
  };

  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) {
          addToken.reset();
          importCert.reset();
          if (signIn.isPending) signIn.cancel();
          signIn.reset();
        }
      }}
    >
      <SheetTrigger asChild>{trigger}</SheetTrigger>
      <SheetContent
        title={t("connect.title")}
        description={t("connect.description")}
        footer={
          <>
            {cert.data ? (
              <Button
                variant="plain"
                className="mr-auto"
                pending={importCert.isPending}
                onClick={() => importCert.mutate(undefined, { onSuccess: (a) => finish([a.name]) })}
              >
                {t("connect.useCert")}
              </Button>
            ) : null}
            <SheetClose asChild>
              <Button>{t("common.cancel")}</Button>
            </SheetClose>
            <Button
              variant="primary"
              type="submit"
              form="connect-token"
              disabled={token.trim() === ""}
              pending={addToken.isPending}
            >
              {addToken.isPending ? t("connect.checking") : t("connect.connect")}
            </Button>
          </>
        }
      >
        {oauthAvailable ? (
          <div className="mb-5 flex flex-col gap-3 border-separator border-b-hairline pb-5">
            {signIn.isPending ? (
              <div className="flex flex-col gap-2" aria-live="polite">
                <div className="flex items-center gap-2 text-body">
                  <Spinner className="size-3.5" /> {t("connect.waiting")}
                </div>
                {signIn.url ? (
                  <CopyField label={t("connect.signInLink")} value={signIn.url} />
                ) : null}
                <div>
                  <Button size="sm" onClick={signIn.cancel}>
                    {t("connect.cancelSignIn")}
                  </Button>
                </div>
              </div>
            ) : (
              <Button
                variant="primary"
                size="lg"
                className="self-start"
                onClick={() =>
                  signIn.mutate(undefined, { onSuccess: (a) => finish(a.map((x) => x.name)) })
                }
              >
                {t("connect.signIn")}
              </Button>
            )}
            {signIn.error && toIpcError(signIn.error).key !== "core.app.signInCancelled" ? (
              <p role="alert" className="text-callout text-error">
                {toIpcError(signIn.error).message}
              </p>
            ) : null}
            <p className="text-callout text-secondary">{t("connect.orToken")}</p>
          </div>
        ) : null}
        <form id="connect-token" onSubmit={submit}>
          <ol className="flex flex-col gap-5">
            <Step n={1} title={t("connect.step1")}>
              <p className="text-callout text-secondary">{t("connect.step1Detail")}</p>
              <div>
                <Button onClick={() => void openTokenPage()}>
                  {t("connect.openCloudflare")} <ExternalLink />
                </Button>
              </div>
            </Step>
            <Step n={2} title={t("connect.step2")}>
              <Field label={t("connect.token")} error={message}>
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
                  />
                )}
              </Field>
            </Step>
          </ol>
        </form>
      </SheetContent>
    </Sheet>
  );
}
