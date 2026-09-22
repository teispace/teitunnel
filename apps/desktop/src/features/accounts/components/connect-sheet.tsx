import { ExternalLink } from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Sheet, SheetClose, SheetContent, SheetTrigger } from "@/components/ui/sheet";
import { toIpcError } from "@/lib/ipc/client";
import { openTokenPage, useAddToken, useCertDetected, useImportCert } from "../queries";

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
  const error = addToken.error ?? importCert.error;
  const message = error ? toIpcError(error).message : null;

  const finish = (names: string[]) => {
    setOpen(false);
    setToken("");
    toast.success(
      names.length === 1 ? `Connected ${names[0]}` : `Connected ${names.length} accounts`,
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
        }
      }}
    >
      <SheetTrigger asChild>{trigger}</SheetTrigger>
      <SheetContent
        title="Connect Cloudflare"
        description="Teitunnel uses an API token you create in your Cloudflare dashboard. It's kept in your Mac's keychain and only sent to Cloudflare."
        footer={
          <>
            {cert.data ? (
              <Button
                variant="plain"
                className="mr-auto"
                disabled={importCert.isPending}
                onClick={() => importCert.mutate(undefined, { onSuccess: (a) => finish([a.name]) })}
              >
                Use my cloudflared login
              </Button>
            ) : null}
            <SheetClose asChild>
              <Button>Cancel</Button>
            </SheetClose>
            <Button
              variant="primary"
              type="submit"
              form="connect-token"
              disabled={token.trim() === "" || addToken.isPending}
            >
              {addToken.isPending ? "Checking…" : "Connect"}
            </Button>
          </>
        }
      >
        <form id="connect-token" onSubmit={submit}>
          <ol className="flex flex-col gap-5">
            <Step n={1} title="Create a token">
              <p className="text-callout text-secondary">
                The link opens Cloudflare with the right permissions selected. If “Cloudflare Tunnel
                · Edit” isn't in the list, add it, then choose Create Token.
              </p>
              <div>
                <Button onClick={() => void openTokenPage()}>
                  Open Cloudflare <ExternalLink />
                </Button>
              </div>
            </Step>
            <Step n={2} title="Paste it here">
              <Field label="API token" error={message}>
                {(control) => (
                  <Input
                    {...control}
                    type="password"
                    autoComplete="off"
                    placeholder="Paste your token"
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
