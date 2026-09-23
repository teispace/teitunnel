import { commands } from "@/lib/ipc/bindings";
import { isTauri } from "./platform";

const BENIGN = /^ResizeObserver loop/;

/** Forwards uncaught errors and rejections to the app log, so they appear in diagnostics. */
export function installErrorReporting(): void {
  if (!isTauri()) return;
  const report = (error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    const stack = error instanceof Error ? (error.stack ?? null) : null;
    void commands.appReportError(message, stack).catch(() => {});
  };
  window.addEventListener("error", (event) => {
    // A browser notice, not a failure: an observer resized something in its own callback
    // and the rest was delivered next frame. WebKit repeats it every frame while it lasts.
    if (BENIGN.test(event.message)) return;
    report(event.error ?? event.message);
  });
  window.addEventListener("unhandledrejection", (event) => report(event.reason));
}
