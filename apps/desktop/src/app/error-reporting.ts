import { commands } from "@/lib/ipc/bindings";
import { isTauri } from "./platform";

/** Forwards uncaught errors and rejections to the app log, so they appear in diagnostics. */
export function installErrorReporting(): void {
  if (!isTauri()) return;
  const report = (error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    const stack = error instanceof Error ? (error.stack ?? null) : null;
    void commands.appReportError(message, stack).catch(() => {});
  };
  window.addEventListener("error", (event) => report(event.error ?? event.message));
  window.addEventListener("unhandledrejection", (event) => report(event.reason));
}
