import { LogIn } from "lucide-react";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { ConnectSheet } from "./connect-sheet";

/**
 * The way out of a failed load: Sign In Again when the account's Cloudflare sign-in
 * ended (retrying can't help), Try Again otherwise.
 */
export function ErrorAction({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  if (error && toIpcError(error).key === "core.error.oauth.signedOut") {
    return (
      <ConnectSheet
        trigger={
          <Button variant="primary">
            <LogIn /> {t("accounts.signInAgain")}
          </Button>
        }
      />
    );
  }
  return <Button onClick={onRetry}>{t("common.tryAgain")}</Button>;
}
