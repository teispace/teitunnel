import { useState } from "react";
import { InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { PermissionFix } from "@/features/accounts";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { botLabels, complete, describeHeaders, describeRateLimit } from "../model";
import { useProtection } from "../queries";
import { ProtectionSheet } from "./protection-sheet";
import { QuotaList } from "./quota-list";

const PERMISSION_KEYS = ["core.error.observe.edgePermission", "core.error.cloudflare.permission"];

/**
 * A hostname's rules at Cloudflare's edge (bots, AI crawlers, rate limit, headers) with
 * the zone's quotas, and the sheet that changes them. Enforced by Cloudflare, so they
 * work while this computer is off.
 */
export function ProtectionSection({
  accountId,
  hostname,
}: {
  accountId: string;
  hostname: string;
}) {
  const [editing, setEditing] = useState(false);
  const query = useProtection(accountId, hostname);
  const error = query.error ? toIpcError(query.error) : null;
  const protection = complete(query.data?.protection);

  return (
    <InspectorSection title={t("protection.title")}>
      <p className="text-callout text-secondary">{t("protection.where")}</p>
      {query.isPending ? (
        <div className="flex flex-col gap-2" aria-busy>
          <Skeleton className="h-4 w-3/4" />
          <Skeleton className="h-4 w-1/2" />
          <Skeleton className="h-4 w-2/3" />
        </div>
      ) : error && PERMISSION_KEYS.includes(error.key ?? "") ? (
        <PermissionFix
          accountId={accountId}
          needs={[{ kind: "edgeRules" }]}
          refused
          onReady={() => void query.refetch()}
        />
      ) : error ? (
        <p role="alert" className="text-callout text-error">
          {error.message}
        </p>
      ) : (
        <>
          <KeyValueGrid
            items={[
              { label: t("protection.bots.label"), value: t(botLabels[protection.bots]) },
              {
                label: t("protection.ai.short"),
                value: protection.aiCrawlers
                  ? t("protection.ai.blocked")
                  : t("protection.ai.allowed"),
              },
              {
                label: t("protection.rateLimit.label"),
                value: describeRateLimit(protection.rateLimit),
              },
              { label: t("protection.headers.label"), value: describeHeaders(protection) },
            ]}
          />
          {query.data ? <QuotaList quotas={query.data.quotas} zone={query.data.zone} /> : null}
        </>
      )}
      <div>
        <Button
          size="sm"
          disabled={query.isPending || error !== null}
          onClick={() => setEditing(true)}
        >
          {t("protection.edit")}
        </Button>
      </div>
      <ProtectionSheet
        accountId={accountId}
        hostname={hostname}
        current={query.data}
        open={editing}
        onClose={() => setEditing(false)}
      />
    </InspectorSection>
  );
}
