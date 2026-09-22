import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "./platform";

export type ThemePreference = "system" | "light" | "dark";

/**
 * Applies the appearance override. The web layer pins `color-scheme` via `data-theme`;
 * the native window follows too, so the sidebar vibrancy and traffic lights match.
 */
export async function applyTheme(
  preference: ThemePreference,
  root: HTMLElement = document.documentElement,
): Promise<void> {
  if (preference === "system") delete root.dataset["theme"];
  else root.dataset["theme"] = preference;
  if (isTauri()) {
    await getCurrentWindow().setTheme(preference === "system" ? null : preference);
  }
}
