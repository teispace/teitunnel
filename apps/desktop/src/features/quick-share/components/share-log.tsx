import { LogLines } from "@/components/patterns/log-lines";
import { useShareLogs } from "../queries";

/** A share's newest cloudflared log lines. */
export function ShareLog({ id }: { id: string }) {
  const { data: lines = [] } = useShareLogs(id, true);
  return <LogLines lines={lines} empty="No log output yet." />;
}
