import {
  Action,
  ActionPanel,
  Alert,
  confirmAlert,
  Icon,
  List,
  LaunchType,
  launchCommand,
  showToast,
  Toast,
  Keyboard,
} from "@raycast/api";
import { usePromise } from "@raycast/utils";
import type { ShareInfo } from "./control-client/index.ts";
import { shortUrl } from "./control-client/index.ts";
import { shareRow, sortShares } from "./logic.ts";
import { explain, openTeitunnel, withTeitunnel } from "./teitunnel.ts";

function icon(share: ShareInfo) {
  if (share.status === "live") return Icon.Globe;
  if (share.status === "failed") return Icon.Warning;
  return Icon.CircleProgress;
}

/** Every share, with copy, open, requests and stop. */
export default function ListShares() {
  const { data, isLoading, error, revalidate } = usePromise(
    () => withTeitunnel((client) => client.listShares()),
    [],
    { onError: (e) => void explain(e) },
  );

  const stop = async (share: ShareInfo) => {
    const confirmed = await confirmAlert({
      title: "Stop sharing?",
      message: `${share.url ? shortUrl(share.url) : share.origin} stops working for everyone who has it.`,
      primaryAction: { title: "Stop Share", style: Alert.ActionStyle.Destructive },
    });
    if (!confirmed) return;
    try {
      await withTeitunnel((client) => client.stopShare(share.id));
      await showToast({ style: Toast.Style.Success, title: `Stopped ${shortUrl(share.origin)}` });
      revalidate();
    } catch (e) {
      await explain(e);
    }
  };

  const openInApp = (task: Parameters<typeof withTeitunnel>[0]) =>
    withTeitunnel(task).catch((e: unknown) => explain(e));

  const shareNew = () =>
    launchCommand({ name: "share-port", type: LaunchType.UserInitiated }).catch(() => {});

  return (
    <List isLoading={isLoading} searchBarPlaceholder="Filter shares">
      {error ? (
        <List.EmptyView
          icon={Icon.Plug}
          title="Teitunnel isn't running"
          description={error.message}
          actions={
            <ActionPanel>
              <Action title="Open Teitunnel" icon={Icon.AppWindow} onAction={openTeitunnel} />
              <Action title="Try Again" icon={Icon.ArrowClockwise} onAction={revalidate} />
            </ActionPanel>
          }
        />
      ) : (
        <List.EmptyView
          icon={Icon.Globe}
          title="Nothing is shared"
          description="Share a local port to get a public address."
          actions={
            <ActionPanel>
              <Action title="Share Port" icon={Icon.Plus} onAction={shareNew} />
            </ActionPanel>
          }
        />
      )}
      {sortShares(data ?? []).map((share) => {
        const row = shareRow(share);
        return (
          <List.Item
            key={share.id}
            icon={icon(share)}
            title={row.title}
            subtitle={row.subtitle}
            keywords={row.keywords}
            accessories={[{ text: row.status }]}
            actions={
              <ActionPanel>
                {share.url ? (
                  <>
                    <Action.CopyToClipboard title="Copy Address" content={share.url} />
                    <Action.OpenInBrowser url={share.url} />
                  </>
                ) : null}
                <Action
                  title="Open Inspector"
                  icon={Icon.MagnifyingGlass}
                  shortcut={{
                    macOS: { modifiers: ["cmd"], key: "i" },
                    Windows: { modifiers: ["ctrl"], key: "i" },
                  }}
                  onAction={() =>
                    openInApp((client) => client.open({ view: "inspector", share: share.id }))
                  }
                />
                <Action
                  title="Show in Teitunnel"
                  icon={Icon.AppWindow}
                  shortcut={Keyboard.Shortcut.Common.Open}
                  onAction={() =>
                    openInApp((client) => client.open({ view: "share", id: share.id }))
                  }
                />
                <Action
                  title="Stop Share"
                  icon={Icon.Stop}
                  style={Action.Style.Destructive}
                  shortcut={Keyboard.Shortcut.Common.Remove}
                  onAction={() => stop(share)}
                />
                <Action
                  title="Refresh"
                  icon={Icon.ArrowClockwise}
                  shortcut={Keyboard.Shortcut.Common.Refresh}
                  onAction={revalidate}
                />
              </ActionPanel>
            }
          />
        );
      })}
    </List>
  );
}
