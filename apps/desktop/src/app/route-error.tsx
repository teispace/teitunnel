import type { ErrorComponentProps } from "@tanstack/react-router";
import { ErrorState } from "@/components/patterns/error-state";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";

/** Shown when a view fails to load or render. The rest of the window keeps working. */
export function RouteError({ error, reset }: ErrorComponentProps) {
  const { message, hint } = toIpcError(error);
  return (
    <ErrorState
      title={t("error.viewFailed")}
      message={message}
      hint={hint}
      action={<Button onClick={reset}>{t("common.tryAgain")}</Button>}
    />
  );
}
