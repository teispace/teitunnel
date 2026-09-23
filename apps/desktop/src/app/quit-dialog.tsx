import { useMutation } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent } from "@/components/ui/dialog";
import { t } from "@/lib/i18n";
import { commands } from "@/lib/ipc/bindings";
import { call, toIpcError } from "@/lib/ipc/client";
import { useUiStore } from "./ui-store";

/**
 * Asked on ⌘Q while routes run through the app's connector: keep them up as a service
 * (Always-on), quit anyway, or stay.
 */
export function QuitDialog() {
  const open = useUiStore((state) => state.quitOpen);
  const setOpen = useUiStore((state) => state.setQuitOpen);
  const quit = useMutation({
    mutationFn: (keepRunning: boolean) => call(commands.appQuit(keepRunning)),
  });
  return (
    <Dialog open={open} onOpenChange={(next) => !quit.isPending && setOpen(next)}>
      <DialogContent
        title={t("quit.title")}
        description={t("quit.description")}
        footer={
          <>
            <DialogClose asChild>
              <Button disabled={quit.isPending}>{t("common.cancel")}</Button>
            </DialogClose>
            <Button disabled={quit.isPending} onClick={() => quit.mutate(false)}>
              {t("quit.anyway")}
            </Button>
            <Button variant="primary" disabled={quit.isPending} onClick={() => quit.mutate(true)}>
              {quit.isPending && quit.variables ? t("quit.switching") : t("quit.keep")}
            </Button>
          </>
        }
      >
        {quit.error ? (
          <p role="alert" className="text-callout text-error">
            {toIpcError(quit.error).message}
          </p>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
