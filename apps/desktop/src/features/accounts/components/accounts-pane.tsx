import { Plus } from "lucide-react";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { t } from "@/lib/i18n";
import type { Account } from "@/lib/ipc/bindings";
import { useAccounts, useRemoveAccount } from "../queries";
import { CapabilityList } from "./capability-list";
import { ConnectSheet } from "./connect-sheet";

export function credentialLabel(account: Account): string {
  switch (account.credential) {
    case "apiToken":
      return t("accounts.credential.apiToken");
    case "oauth":
      return t("accounts.credential.oauth");
    case "certPem":
      return t("accounts.credential.certPem");
  }
}

function RemoveButton({ account }: { account: Account }) {
  const remove = useRemoveAccount();
  return (
    <ConfirmDialog
      trigger={
        <Button size="sm" variant="destructive">
          {t("accounts.disconnect")}
        </Button>
      }
      title={t("accounts.disconnectTitle", { name: account.name })}
      description={t("accounts.disconnectDetail")}
      confirmLabel={t("accounts.disconnect")}
      onConfirm={() => remove.mutateAsync(account.id)}
    />
  );
}

/** Settings → Accounts. */
export function AccountsPane() {
  const { data: accounts = [], isSuccess } = useAccounts();
  if (!isSuccess) return <SkeletonSection rows={2} />;
  return (
    <GroupedSection title={t("accounts.title")} footer={t("accounts.footer")}>
      {accounts.length === 0 ? (
        <GroupedRow label={t("accounts.none")} description={t("accounts.noneDetail")} />
      ) : null}
      {accounts.map((account) => (
        <div key={account.id} className="flex flex-col gap-1 py-2">
          <div className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <div className="truncate text-body">{account.name}</div>
              <div className="text-callout text-secondary">{credentialLabel(account)}</div>
            </div>
            <RemoveButton account={account} />
          </div>
          <Disclosure
            title={<span className="text-callout font-normal text-secondary">Permissions</span>}
          >
            <CapabilityList accountId={account.id} />
          </Disclosure>
        </div>
      ))}
      <div className="py-2">
        <ConnectSheet
          trigger={
            <Button size="sm">
              <Plus /> Connect an Account
            </Button>
          }
        />
      </div>
    </GroupedSection>
  );
}
