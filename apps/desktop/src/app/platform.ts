import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands } from "@/lib/ipc/bindings";

export type Platform = "macos" | "windows" | "linux";

/** Best-effort platform detection before `app_info` resolves (for first-paint chrome). */
export function detectPlatform(): Platform {
  const ua = navigator.userAgent;
  if (ua.includes("Mac")) return "macos";
  if (ua.includes("Windows")) return "windows";
  return "linux";
}

/**
 * Mirrors platform and window focus onto <html> (`data-platform`, `data-window-active`)
 * so CSS can render the inactive-window state. Returns a cleanup function.
 */
export function syncWindowChrome(root: HTMLElement = document.documentElement): () => void {
  root.dataset["platform"] = detectPlatform();
  root.dataset["windowActive"] = String(document.hasFocus());

  const setActive = (active: boolean) => {
    root.dataset["windowActive"] = String(active);
    // The accent can only change in System Settings, so re-reading it whenever the
    // window regains focus keeps it current without observing AppKit notifications.
    if (active) void applySystemAccent(root);
  };
  void applySystemAccent(root);
  const onFocus = () => setActive(true);
  const onBlur = () => setActive(false);
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onBlur);

  // The webview's own focus events miss some cases (e.g. clicking the title bar of
  // another window), so the native focus signal is authoritative when available.
  const unlisten = isTauri()
    ? getCurrentWindow().onFocusChanged(({ payload }) => setActive(payload))
    : Promise.resolve(() => {});

  return () => {
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("blur", onBlur);
    void unlisten.then((stop) => stop());
  };
}

/** True inside the Tauri webview (false in unit tests and plain browsers). */
export function isTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

/** Sets `--accent` to the system accent colour reported by the shell (D-023). */
export async function applySystemAccent(root: HTMLElement): Promise<void> {
  if (!isTauri()) return;
  try {
    const accent = await commands.appAccentColor();
    if (accent) root.style.setProperty("--accent", accent);
    else root.style.removeProperty("--accent");
  } catch (error) {
    console.warn("could not read the system accent colour", error);
  }
}
