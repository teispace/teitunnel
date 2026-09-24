import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { t } from "@/lib/i18n";
import type { IntegrationsPatch } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useIntegrations, useRevokeClient, useUpdateIntegrations } from "./queries";

const day = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });

/** Settings ▸ Integrations: the control connection, links and always-allowed programs. */
export function IntegrationsPane() {
  const { data } = useIntegrations();
  const update = useUpdateIntegrations();
  const revoke = useRevokeClient();
  if (!data) {
    return (
      <>
        <SkeletonSection />
        <SkeletonSection rows={2} />
        <SkeletonSection />
      </>
    );
  }
  // Show the switch where it's going while the change is saved.
  const pending = update.isPending ? update.variables : undefined;
  const value = (key: keyof IntegrationsPatch) => pending?.[key] ?? data[key];
  const error = update.error ?? revoke.error;
  return (
    <>
      <GroupedSection
        title={t("integrations.control.title")}
        footer={t("integrations.control.footer")}
      >
        <GroupedRow
          label={t("integrations.control.enabled")}
          description={t("integrations.control.enabledDetail")}
        >
          <Switch
            aria-label={t("integrations.control.enabled")}
            checked={value("controlEnabled")}
            disabled={update.isPending}
            onCheckedChange={(controlEnabled) => update.mutate({ controlEnabled })}
          />
        </GroupedRow>
      </GroupedSection>
      <GroupedSection
        title={t("integrations.clients.title")}
        footer={t("integrations.clients.footer")}
      >
        {data.clients.length === 0 ? (
          <p className="py-2 text-callout text-secondary">{t("integrations.clients.empty")}</p>
        ) : (
          [...data.clients].reverse().map((client) => {
            const busy = revoke.isPending && revoke.variables === client.name;
            return (
              <GroupedRow
                key={client.name}
                label={client.name}
                description={t("integrations.clients.approved", {
                  date: day.format(new Date(client.approvedAt)),
                })}
              >
                <Button
                  size="sm"
                  pending={busy}
                  disabled={revoke.isPending && !busy}
                  aria-label={t("integrations.clients.revokeLabel", { name: client.name })}
                  onClick={() => revoke.mutate(client.name)}
                >
                  {t("integrations.clients.revoke")}
                </Button>
              </GroupedRow>
            );
          })
        )}
      </GroupedSection>
      <GroupedSection title={t("integrations.links.title")} footer={t("integrations.links.footer")}>
        <GroupedRow
          label={t("integrations.links.enabled")}
          description={t("integrations.links.enabledDetail")}
        >
          <Switch
            aria-label={t("integrations.links.enabled")}
            checked={value("deepLinksEnabled")}
            disabled={update.isPending}
            onCheckedChange={(deepLinksEnabled) => update.mutate({ deepLinksEnabled })}
          />
        </GroupedRow>
      </GroupedSection>
      {error ? (
        <p role="alert" className="text-callout text-error">
          {t("integrations.error", { message: toIpcError(error).message })}
        </p>
      ) : null}
    </>
  );
}
