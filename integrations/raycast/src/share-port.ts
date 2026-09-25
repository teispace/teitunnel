import { Clipboard, type LaunchProps, showHUD, showToast, Toast } from "@raycast/api";
import { shortUrl } from "./control-client/index.ts";
import { parseOrigin } from "./logic.ts";
import { explain, withTeitunnel } from "./teitunnel.ts";

/** Shares a local port and copies the address (Teitunnel asks first unless Raycast is always allowed). */
export default async function SharePort(props: LaunchProps<{ arguments: { port: string } }>) {
  const origin = parseOrigin(props.arguments.port);
  if (!origin) {
    await showToast({ style: Toast.Style.Failure, title: "Enter a port like 3000" });
    return;
  }
  const toast = await showToast({ style: Toast.Style.Animated, title: `Sharing ${origin}…` });
  try {
    const share = await withTeitunnel((client) => client.startShare({ origin }));
    await toast.hide();
    if (!share.url) return;
    await Clipboard.copy(share.url);
    await showHUD(`Shared ${shortUrl(share.origin)}: address copied`);
  } catch (error) {
    await toast.hide();
    await explain(error, { hud: true });
  }
}
