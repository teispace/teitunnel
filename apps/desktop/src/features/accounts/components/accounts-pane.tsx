import { Plus } from "lucide-react";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogTrigger } from "@/components/ui/dialog";
import { Disclosure } from "@/components/ui/disclosure";
import type { Account } from "@/lib/ipc/bindings";
import { useAccounts, useRemoveAccount } from "../queries";
import { CapabilityList } from "./capability-list";
import { ConnectSheet } from "./connect-sheet";

export function credentialLabel(account: Account): string {
  switch (account.credential) {
    case "apiToken":
      return "API token";
    case "oauth":
      return "Signed in with Cloudflare";
    case "certPem":
      return "cloudflared login · one domain only";
  }
}

function RemoveButton({ account }: { account: Account }) {
  const remove = useRemoveAccount();
  return (
    <Dialog>
      <DialogTrigger asChild>
        <Button size="sm" variant="destructive">
          Disconnect
        </Button>
      </DialogTrigger>
      <DialogContent
        title={`Disconnect ${account.name}?`}
        description="Teitunnel deletes this account's credentials from your keychain. Nothing changes in Cloudflare; running routes keep working until you stop them."
        footer={
          <>
            <DialogClose asChild>
              <Button>Cancel</Button>
            </DialogClose>
            <DialogClose asChild>
              <Button variant="primary" onClick={() => remove.mutate(account.id)}>
                Disconnect
              </Button>
            </DialogClose>
          </>
        }
      />
    </Dialog>
  );
}

/** Settings → Accounts. */
export function AccountsPane() {
  const { data: accounts = [], isSuccess } = useAccounts();
  if (!isSuccess) return null;
  return (
    <GroupedSection
      title="Cloudflare accounts"
      footer="Credentials are stored in your Mac's keychain and only sent to Cloudflare."
    >
      {accounts.length === 0 ? (
        <GroupedRow
          label="No accounts connected"
          description="Connect one to use your own domains."
        />
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
