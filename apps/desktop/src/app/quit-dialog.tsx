import { useMutation } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent } from "@/components/ui/dialog";
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
        title="Your routes stop when Teitunnel quits"
        description="They can keep running in the background instead, and start again when you log in. You can change this in Tunnels."
        footer={
          <>
            <DialogClose asChild>
              <Button disabled={quit.isPending}>Cancel</Button>
            </DialogClose>
            <Button disabled={quit.isPending} onClick={() => quit.mutate(false)}>
              Quit Anyway
            </Button>
            <Button variant="primary" disabled={quit.isPending} onClick={() => quit.mutate(true)}>
              {quit.isPending && quit.variables ? "Switching…" : "Keep Routes Running"}
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
