import { closeMainWindow, open, showHUD, showToast, Toast } from "@raycast/api";
import { ControlClient, ControlError, OPEN_APP_URL } from "./control-client/index.ts";

/** How this extension introduces itself to the app (shown when a change needs approval). */
const CLIENT = { name: "raycast", version: "0.1.0" };

/** Runs `task` on a fresh connection to the app, closed afterwards. */
export async function withTeitunnel<T>(task: (client: ControlClient) => Promise<T>): Promise<T> {
  const client = await ControlClient.connect({ client: CLIENT });
  try {
    return await task(client);
  } finally {
    client.close();
  }
}

/** Brings Teitunnel to the front (starting it if needed). */
export async function openTeitunnel(): Promise<void> {
  await open(OPEN_APP_URL);
  await closeMainWindow();
}

/**
 * Tells the person what went wrong: nothing loud for a change they declined, an
 * "Open Teitunnel" button when the app isn't running, the app's words otherwise.
 */
export async function explain(error: unknown, options: { hud?: boolean } = {}): Promise<void> {
  if (error instanceof ControlError && error.declined) {
    if (options.hud) await showHUD("Not allowed in Teitunnel");
    return;
  }
  const message = error instanceof Error ? error.message : String(error);
  if (error instanceof ControlError && error.appUnavailable) {
    await showToast({
      style: Toast.Style.Failure,
      title: error.kind === "notInstalled" ? "Teitunnel isn't set up" : "Teitunnel isn't running",
      message,
      primaryAction: {
        title: "Open Teitunnel",
        onAction: () => void openTeitunnel(),
      },
    });
    return;
  }
  await showToast({ style: Toast.Style.Failure, title: "Teitunnel", message });
}
