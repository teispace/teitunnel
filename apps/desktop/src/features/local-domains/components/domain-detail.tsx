import { ExternalLink, Smartphone } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { CopyField } from "@/components/patterns/copy-field";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-dot";
import { Switch } from "@/components/ui/switch";
import { t } from "@/lib/i18n";
import type { CaFormat, LocalDomainsStatus, LocalDomainView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { domainState, suffixOf, targetText } from "../model";
import { saveCaCertificate, useRemoveLocalDomain, useSetInspect, useSetLan } from "../queries";
import { DomainSheet } from "./domain-sheet";

/** Phones on the same network: `.local` names, LAN access and the CA to install. */
function PhoneAccess({ domain, status }: { domain: LocalDomainView; status: LocalDomainsStatus }) {
  const setLan = useSetLan();
  const [saving, setSaving] = useState<CaFormat | null>(null);
  const save = async (format: CaFormat) => {
    setSaving(format);
    try {
      const path = await saveCaCertificate(format);
      if (path) toast.success(t("localDomains.phone.saved", { path }));
    } catch (error) {
      toast.error(toIpcError(error).message);
    } finally {
      setSaving(null);
    }
  };
  if (suffixOf(domain.name) !== "local") {
    return (
      <p className="text-callout text-secondary">
        <Smartphone aria-hidden className="mr-1 inline size-3.5 align-[-2px]" />
        {t("localDomains.phone.useLocal")}
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      <GroupedSection footer={t("localDomains.phone.lanFooter")}>
        <GroupedRow
          label={t("localDomains.phone.lan")}
          description={
            status.lanAddresses.length > 0
              ? t("localDomains.phone.addresses", { addresses: status.lanAddresses.join(", ") })
              : t("localDomains.phone.noNetwork")
          }
        >
          <Switch
            aria-label={t("localDomains.phone.lan")}
            checked={status.lan}
            disabled={setLan.isPending}
            onCheckedChange={(lan) => setLan.mutate(lan)}
          />
        </GroupedRow>
      </GroupedSection>
      <p className="text-callout text-secondary">{t("localDomains.phone.install")}</p>
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          onClick={() => void save("appleProfile")}
          pending={saving === "appleProfile"}
        >
          {t("localDomains.phone.saveProfile")}
        </Button>
        <Button
          size="sm"
          onClick={() => void save("certificate")}
          pending={saving === "certificate"}
        >
          {t("localDomains.phone.saveCertificate")}
        </Button>
      </div>
    </div>
  );
}

interface DomainDetailProps {
  domain: LocalDomainView;
  status: LocalDomainsStatus;
  trusted: boolean | undefined;
}

/** One local domain: its address, service, options, inspection, phones, and Remove. */
export function DomainDetail({ domain, status, trusted }: DomainDetailProps) {
  const state = domainState(domain, trusted);
  const inspect = useSetInspect();
  const remove = useRemoveLocalDomain();
  const [editing, setEditing] = useState(false);
  const yes = t("localDomains.detail.yes");
  const no = t("localDomains.detail.no");

  return (
    <Inspector
      title={domain.name}
      subtitle={
        <span className="flex items-center gap-1.5">
          <StatusDot status={state.dot} label={state.label} /> {state.label}
        </span>
      }
      actions={
        <>
          <Button
            variant="primary"
            disabled={!domain.serving}
            onClick={() => void openUrl(domain.url)}
          >
            {t("localDomains.detail.open")} <ExternalLink />
          </Button>
          <Button onClick={() => setEditing(true)}>{t("localDomains.detail.edit")}</Button>
        </>
      }
    >
      <InspectorSection title={t("localDomains.detail.address")}>
        <CopyField label={t("localDomains.detail.copyUrl")} value={domain.url} />
        {domain.wildcard ? (
          <p className="mt-1.5 text-callout text-secondary">
            {t("localDomains.detail.wildcardNote", { name: domain.name })}
          </p>
        ) : null}
      </InspectorSection>
      <InspectorSection title={t("localDomains.detail.details")}>
        <KeyValueGrid
          items={[
            {
              label: t("localDomains.detail.service"),
              value: targetText(domain.target, domain.origin),
              mono: true,
            },
            { label: t("localDomains.detail.https"), value: domain.https ? yes : no },
            { label: t("localDomains.detail.wildcard"), value: domain.wildcard ? yes : no },
            ...(domain.project
              ? [{ label: t("localDomains.detail.project"), value: domain.project, mono: true }]
              : []),
          ]}
        />
      </InspectorSection>
      <InspectorSection title={t("localDomains.detail.inspector")}>
        <GroupedSection>
          <GroupedRow
            label={t("localDomains.detail.inspect")}
            description={t("localDomains.detail.requests", { count: domain.requests ?? 0 })}
          >
            <Switch
              aria-label={t("localDomains.detail.inspect")}
              checked={domain.inspect}
              disabled={inspect.isPending}
              onCheckedChange={(value) => inspect.mutate({ name: domain.name, inspect: value })}
            />
          </GroupedRow>
        </GroupedSection>
      </InspectorSection>
      <InspectorSection title={t("localDomains.detail.phones")}>
        <PhoneAccess domain={domain} status={status} />
      </InspectorSection>
      <div>
        <ConfirmDialog
          trigger={<Button variant="destructive">{t("localDomains.detail.remove")}</Button>}
          title={t("localDomains.detail.removeTitle", { name: domain.name })}
          description={t("localDomains.detail.removeDetail")}
          confirmLabel={t("localDomains.detail.removeConfirm")}
          variant="destructive"
          onConfirm={() => remove.mutateAsync(domain.name)}
        />
      </div>
      {editing ? <DomainSheet open editing={domain} onClose={() => setEditing(false)} /> : null}
    </Inspector>
  );
}
