import { LogViewer } from "@/components/patterns/log-viewer";
import { Button } from "@/components/ui/button";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import type { ConnectorView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { type RemoteConnector, useRemoteLogs, useSaveLog } from "../queries";

interface RemoteLogsSheetProps {
  /** The connector whose logs to show; `null` closes the sheet. */
  target: (RemoteConnector & { connector: ConnectorView }) | null;
  onClose: () => void;
}

/**
 * Live logs of a connector on another machine, streamed through Cloudflare for as long
 * as the sheet is open.
 */
export function RemoteLogsSheet({ target, onClose }: RemoteLogsSheetProps) {
  const logs = useRemoteLogs(target);
  const save = useSaveLog();
  const state = logs.data?.state;
  const error = logs.error ? toIpcError(logs.error) : null;
  const status =
    error?.message ??
    (state?.state === "ended"
      ? state.message
      : state?.state === "streaming"
        ? null
        : "Connecting to the connector…");

  return (
    <Sheet open={target !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        width="lg"
        title="Connector Logs"
        description={
          target
            ? `${target.connector.originIp} · cloudflared ${target.connector.version}. Relayed live by Cloudflare while this is open.`
            : undefined
        }
        footer={
          <>
            {state?.state === "ended" || error ? (
              <Button className="mr-auto" onClick={() => void logs.retry()}>
                Try Again
              </Button>
            ) : null}
            <SheetClose asChild>
              <Button variant="primary">Done</Button>
            </SheetClose>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          {status ? (
            <p
              role={state?.state === "ended" || error ? "alert" : "status"}
              className={
                state?.state === "ended" || error
                  ? "text-callout text-error"
                  : "flex items-center gap-2 text-callout text-secondary"
              }
            >
              {state?.state === "ended" || error ? null : <Spinner className="size-3.5" />}
              {status}
            </p>
          ) : null}
          <LogViewer
            lines={logs.data?.lines ?? []}
            height={360}
            empty="Nothing logged yet. Connection changes and errors appear here as they happen."
            onSave={save}
          />
        </div>
      </SheetContent>
    </Sheet>
  );
}
