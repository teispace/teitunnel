import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { InspectorSection } from "@/components/patterns/inspector";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { PermissionFix } from "@/features/accounts";
import { t, translate } from "@/lib/i18n";
import type { IssuedTokenView, ServiceTokenView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { applyProtectionDirectly, useServiceTokens } from "../queries";
import { TokenSheet } from "./token-sheet";

const PERMISSION_KEYS = [
  "core.error.observe.serviceTokenPermission",
  "core.error.cloudflare.permission",
];

function expiry(token: ServiceTokenView): string {
  if (!token.expiresAt) return t("serviceTokens.noExpiry");
  const date = new Date(token.expiresAt);
  return t("serviceTokens.expires", {
    date: date.toLocaleDateString(undefined, { dateStyle: "medium" }),
  });
}

/** Applies a token change right away (the person confirmed it in the dialog). */
async function applyNow(
  accountId: string,
  change: Parameters<typeof applyProtectionDirectly>[1],
): Promise<IssuedTokenView[]> {
  const { outcome, issued } = await applyProtectionDirectly(accountId, change);
  if (outcome.type !== "applied") throw new Error(translate(outcome.error));
  return issued;
}

/**
 * Service tokens let machines (CI, scripts, other servers) through the hostname's login
 * with two headers. Listed with their expiry; created with the secret shown once;
 * rotated or revoked after a confirmation.
 */
export function ServiceTokens({ accountId, hostname }: { accountId: string; hostname: string }) {
  const tokens = useServiceTokens(accountId, hostname);
  const [creating, setCreating] = useState(false);
  const [rotated, setRotated] = useState<IssuedTokenView | null>(null);
  const error = tokens.error ? toIpcError(tokens.error) : null;

  return (
    <InspectorSection title={t("serviceTokens.title")}>
      <p className="text-callout text-secondary">{t("serviceTokens.intro")}</p>
      {tokens.isPending ? (
        <div className="flex flex-col gap-2" aria-busy>
          <Skeleton className="h-4 w-2/3" />
          <Skeleton className="h-4 w-1/2" />
        </div>
      ) : error && PERMISSION_KEYS.includes(error.key ?? "") ? (
        <PermissionFix
          accountId={accountId}
          needs={[{ kind: "serviceTokens" }]}
          refused
          onReady={() => void tokens.refetch()}
        />
      ) : error ? (
        <p role="alert" className="text-callout text-error">
          {error.message}
        </p>
      ) : (tokens.data ?? []).length === 0 ? (
        <p className="text-callout text-secondary">{t("serviceTokens.empty")}</p>
      ) : (
        <ul className="flex flex-col rounded-card bg-surface-inset px-3 py-1">
          {(tokens.data ?? []).map((token) => (
            <li
              key={token.id}
              className="flex min-h-10 items-center gap-2 border-inset border-b-hairline py-1.5 last:border-b-0"
            >
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="flex items-center gap-1.5 text-body">
                  {token.label}
                  {token.gone ? <Badge>{t("serviceTokens.gone")}</Badge> : null}
                </span>
                <span className="selectable truncate font-mono text-mono text-secondary">
                  {token.clientId}
                </span>
                <span className="text-callout text-secondary">{expiry(token)}</span>
              </div>
              {token.gone ? null : (
                <ConfirmDialog
                  trigger={<Button size="sm">{t("serviceTokens.rotate")}</Button>}
                  title={t("serviceTokens.rotateTitle", { name: token.label })}
                  description={t("serviceTokens.rotateDescription")}
                  confirmLabel={t("serviceTokens.rotate")}
                  onConfirm={async () => {
                    const issued = await applyNow(accountId, {
                      type: "rotateToken",
                      hostname,
                      tokenId: token.id,
                    });
                    setRotated(issued[0] ?? null);
                  }}
                />
              )}
              <ConfirmDialog
                trigger={
                  <Button size="sm" variant="destructive">
                    {t("serviceTokens.revoke")}
                  </Button>
                }
                title={t("serviceTokens.revokeTitle", { name: token.label })}
                description={t("serviceTokens.revokeDescription", { hostname })}
                confirmLabel={t("serviceTokens.revoke")}
                variant="destructive"
                onConfirm={async () => {
                  await applyNow(accountId, { type: "revokeToken", hostname, tokenId: token.id });
                  toast.success(t("serviceTokens.revoked", { name: token.label }));
                  await tokens.refetch();
                }}
              />
            </li>
          ))}
        </ul>
      )}
      <div>
        <Button size="sm" disabled={error !== null} onClick={() => setCreating(true)}>
          {t("serviceTokens.new")}
        </Button>
      </div>
      <TokenSheet
        accountId={accountId}
        hostname={hostname}
        open={creating || rotated !== null}
        issued={rotated}
        onClose={() => {
          setCreating(false);
          setRotated(null);
        }}
      />
    </InspectorSection>
  );
}
