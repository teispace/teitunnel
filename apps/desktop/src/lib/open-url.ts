import { openUrl as tauriOpenUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "@/app/platform";

/** Opens an https:// or mailto: link in the default browser. */
export async function openUrl(url: string): Promise<void> {
  if (isTauri()) await tauriOpenUrl(url);
  else window.open(url, "_blank", "noopener");
}
