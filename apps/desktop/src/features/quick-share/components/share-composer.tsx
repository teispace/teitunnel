import { type FormEvent, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { useActiveAccount, useDomains } from "@/features/accounts";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useStartDomainShare, useStartShare } from "../queries";
import { ServicePicker } from "./service-picker";

const AUTO_STOPS = ["never", "15", "60", "480"] as const;
type AutoStop = (typeof AUTO_STOPS)[number];
const autoStops = () =>
  AUTO_STOPS.map((value) => ({ value, label: t(`quickShare.autoStop.${value}`) }));

/** A random trycloudflare.com address, or a subdomain of one of the account's domains. */
const RANDOM = "random";

/** Origin field + where + auto-stop + one primary action. */
export function ShareComposer({
  disabled = false,
  autoFocus = false,
}: {
  disabled?: boolean;
  autoFocus?: boolean;
}) {
  const [origin, setOrigin] = useState("");
  const [autoStop, setAutoStop] = useState<AutoStop>("never");
  const [address, setAddress] = useState<string>(RANDOM);
  const [subdomain, setSubdomain] = useState("");
  const account = useActiveAccount();
  const domains = (useDomains(account?.id ?? null).data ?? []).filter((d) => d.status === "active");
  const start = useStartShare();
  const startOnDomain = useStartDomainShare();
  const onDomain = address !== RANDOM && account !== null;
  const pending = start.isPending || startOnDomain.isPending;
  const errorId = useId();
  const failure = onDomain ? startOnDomain.error : start.error;
  const error = failure ? toIpcError(failure) : null;
  const fieldError = error?.field === "origin" ? error.message : null;
  const stopAfterMinutes = autoStop === "never" ? null : Number(autoStop);

  const reset = () => {
    if (start.error) start.reset();
    if (startOnDomain.error) startOnDomain.reset();
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (disabled || pending) return;
    if (onDomain && account) {
      const label = subdomain.trim().replace(/\.$/, "");
      startOnDomain.mutate(
        {
          accountId: account.id,
          hostname: label ? `${label}.${address}` : address,
          origin,
          stopAfterMinutes,
        },
        {
          onSuccess: () => {
            setOrigin("");
            setSubdomain("");
          },
        },
      );
    } else {
      start.mutate({ origin, stopAfterMinutes }, { onSuccess: () => setOrigin("") });
    }
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-1.5" noValidate>
      <div className="flex items-center gap-2">
        <ServicePicker
          value={origin}
          onChange={(value) => {
            setOrigin(value);
            reset();
          }}
          autoFocus={autoFocus}
          invalid={fieldError !== null}
          describedBy={fieldError ? errorId : undefined}
        />
        <Select
          label={t("quickShare.whenToStop")}
          options={autoStops()}
          value={autoStop}
          onValueChange={setAutoStop}
          className="h-7"
        />
        <Button type="submit" variant="primary" size="lg" disabled={disabled} pending={pending}>
          {t("quickShare.share")}
        </Button>
      </div>
      {domains.length > 0 ? (
        <div className="flex items-center gap-2">
          <Select
            label={t("quickShare.address.label")}
            options={[
              { value: RANDOM, label: t("quickShare.address.random") },
              ...domains.map((d) => ({
                value: d.name,
                label: t("quickShare.address.onDomain", { domain: d.name }),
              })),
            ]}
            value={address}
            onValueChange={(value) => {
              setAddress(value);
              reset();
            }}
            className="h-7"
          />
          {onDomain ? (
            <div className="flex min-w-0 flex-1 items-center gap-1">
              <Input
                aria-label={t("quickShare.address.subdomain")}
                placeholder={t("quickShare.address.subdomainPlaceholder")}
                autoComplete="off"
                spellCheck={false}
                className="h-7 min-w-0 flex-1 text-right font-mono text-mono"
                value={subdomain}
                onChange={(event) => {
                  setSubdomain(event.target.value);
                  reset();
                }}
              />
              <span className="shrink-0 font-mono text-mono text-secondary">.{address}</span>
            </div>
          ) : null}
        </div>
      ) : null}
      {error && error.code !== "cloudflaredMissing" ? (
        <p id={errorId} role="alert" className="px-1 text-callout text-error">
          {error.message}
          {error.hint ? <span className="text-secondary"> {error.hint}</span> : null}
        </p>
      ) : null}
    </form>
  );
}
