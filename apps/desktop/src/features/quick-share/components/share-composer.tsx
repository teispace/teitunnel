import { type FormEvent, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { useActiveAccount, useDomains } from "@/features/accounts";
import { ExposureCallout, useExposureGate } from "@/features/exposure";
import { t } from "@/lib/i18n";
import type { FolderShare, HostHeaderChoice } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import {
  useLocalServices,
  useStartDomainFolderShare,
  useStartDomainShare,
  useStartFolderShare,
  useStartShare,
} from "../queries";
import { AddressRow, RANDOM } from "./address-row";
import { ChooseFolderButton, FolderChip, FolderOptions } from "./folder-source";
import { ServicePicker } from "./service-picker";

const AUTO_STOPS = ["never", "15", "60", "480"] as const;
type AutoStop = (typeof AUTO_STOPS)[number];
const autoStops = () =>
  AUTO_STOPS.map((value) => ({ value, label: t(`quickShare.autoStop.${value}`) }));

const HOST_MODES = ["auto", "off", "custom"] as const;
type HostMode = (typeof HOST_MODES)[number];
const hostModes = () =>
  HOST_MODES.map((value) => ({ value, label: t(`quickShare.hostHeader.${value}`) }));

function hostHeaderChoice(mode: HostMode, value: string): HostHeaderChoice {
  return mode === "custom" ? { mode: "set", value } : { mode };
}

/**
 * Origin field (or a folder, chosen or dropped) + where + auto-stop + one primary action.
 * The folder is kept by the page, which also takes folders dropped on it.
 */
export function ShareComposer({
  disabled = false,
  autoFocus = false,
  folder = null,
  onFolderChange = () => {},
}: {
  disabled?: boolean;
  autoFocus?: boolean;
  folder?: FolderShare | null;
  onFolderChange?: (folder: FolderShare | null) => void;
}) {
  const [origin, setOrigin] = useState("");
  const [autoStop, setAutoStop] = useState<AutoStop>("never");
  const [address, setAddress] = useState<string>(RANDOM);
  const [subdomain, setSubdomain] = useState("");
  const [hostMode, setHostMode] = useState<HostMode>("auto");
  const [customHost, setCustomHost] = useState("");
  const [folderError, setFolderError] = useState<string | null>(null);
  const account = useActiveAccount();
  const domains = (useDomains(account?.id ?? null).data ?? []).filter((d) => d.status === "active");
  const services = useLocalServices(false).data ?? [];
  const start = useStartShare();
  const startOnDomain = useStartDomainShare();
  const startFolder = useStartFolderShare();
  const startFolderOnDomain = useStartDomainFolderShare();
  const gate = useExposureGate();
  const onDomain = address !== RANDOM && account !== null;
  const pending =
    start.isPending ||
    startOnDomain.isPending ||
    startFolder.isPending ||
    startFolderOnDomain.isPending ||
    gate.checking;
  const errorId = useId();
  const failure = folder
    ? onDomain
      ? startFolderOnDomain.error
      : startFolder.error
    : onDomain
      ? startOnDomain.error
      : start.error;
  const error = failure ? toIpcError(failure) : null;
  const fieldError = error?.field === "origin" ? error.message : null;
  const stopAfterMinutes = autoStop === "never" ? null : Number(autoStop);
  const hostHeader = hostHeaderChoice(hostMode, customHost);
  // The service's project folder names shares (`{project}`, `{branch}`).
  const service =
    services.find((s) => s.origin === origin.trim() || String(s.port) === origin.trim()) ?? null;
  const nameFolder = folder?.path ?? service?.folder ?? null;
  const nameProject = folder ? null : (service?.project ?? null);

  const reset = () => {
    for (const mutation of [start, startOnDomain, startFolder, startFolderOnDomain]) {
      if (mutation.error) mutation.reset();
    }
    setFolderError(null);
    gate.cancel();
  };
  const label = subdomain.trim().replace(/\.$/, "");
  const hostname = label ? `${label}.${address}` : address;
  const done = () => {
    setOrigin("");
    setSubdomain("");
    onFolderChange(null);
  };

  const shareFolder = (folder: FolderShare) => {
    if (onDomain && account) {
      startFolderOnDomain.mutate(
        { accountId: account.id, hostname, folder, stopAfterMinutes },
        { onSuccess: done },
      );
    } else {
      startFolder.mutate({ folder, stopAfterMinutes }, { onSuccess: done });
    }
  };

  const share = () => {
    if (onDomain && account) {
      startOnDomain.mutate(
        {
          accountId: account.id,
          hostname,
          origin,
          stopAfterMinutes,
          hostHeader,
          folder: service?.folder ?? null,
        },
        { onSuccess: done },
      );
    } else {
      start.mutate({ origin, stopAfterMinutes, hostHeader }, { onSuccess: () => setOrigin("") });
    }
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (disabled || pending) return;
    if (folder) {
      shareFolder(folder);
      return;
    }
    // Checked for leaks first; findings wait for "Share Anyway" (never blocks).
    gate.run(origin, share);
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-1.5" noValidate>
      <div className="flex items-center gap-2">
        {folder ? (
          <FolderChip
            folder={folder}
            onClear={() => {
              onFolderChange(null);
              reset();
            }}
          />
        ) : (
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
        )}
        {folder ? null : (
          <ChooseFolderButton
            onChosen={(chosen) => {
              reset();
              onFolderChange(chosen);
            }}
            onError={setFolderError}
          />
        )}
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
        <AddressRow
          domains={domains.map((d) => d.name)}
          address={address}
          onAddressChange={(value) => {
            setAddress(value);
            reset();
          }}
          subdomain={subdomain}
          onSubdomainChange={(value) => {
            setSubdomain(value);
            reset();
          }}
          folder={nameFolder}
          project={nameProject}
        />
      ) : null}
      <Disclosure
        title={
          <span className="text-callout font-normal text-secondary">
            {t("quickShare.advanced")}
          </span>
        }
        defaultOpen={hostMode !== "auto" || folder !== null}
      >
        {folder ? (
          <FolderOptions folder={folder} onChange={onFolderChange} />
        ) : (
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="text-callout text-secondary">
                {t("quickShare.hostHeader.label")}
              </span>
              <Select
                label={t("quickShare.hostHeader.label")}
                options={hostModes()}
                value={hostMode}
                onValueChange={(value) => {
                  setHostMode(value);
                  reset();
                }}
                className="h-7"
              />
              {hostMode === "custom" ? (
                <Input
                  aria-label={t("quickShare.hostHeader.value")}
                  aria-invalid={error?.field === "hostHeader" || undefined}
                  placeholder={t("quickShare.hostHeader.placeholder")}
                  autoComplete="off"
                  spellCheck={false}
                  className="h-7 min-w-0 flex-1 font-mono text-mono"
                  value={customHost}
                  onChange={(event) => {
                    setCustomHost(event.target.value);
                    reset();
                  }}
                />
              ) : null}
            </div>
            <p className="text-footnote text-secondary">{t("quickShare.hostHeader.help")}</p>
          </div>
        )}
      </Disclosure>
      {gate.report ? (
        <ExposureCallout
          report={gate.report}
          actions={
            <>
              <Button type="button" onClick={gate.cancel}>
                {t("common.cancel")}
              </Button>
              <Button type="button" variant="primary" onClick={gate.proceed}>
                {t("exposure.shareAnyway")}
              </Button>
            </>
          }
        />
      ) : null}
      {folderError ? (
        <p role="alert" className="px-1 text-callout text-error">
          {folderError}
        </p>
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
