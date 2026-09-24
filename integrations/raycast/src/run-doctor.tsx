import { Action, ActionPanel, Color, Icon, List, Keyboard } from "@raycast/api";
import { usePromise } from "@raycast/utils";
import type { DoctorIssue } from "./control-client/index.ts";
import { sortIssues } from "./logic.ts";
import { explain, openTeitunnel, withTeitunnel } from "./teitunnel.ts";

function icon(issue: DoctorIssue) {
  if (issue.severity === "error") return { source: Icon.XMarkCircle, tintColor: Color.Red };
  if (issue.severity === "warning") return { source: Icon.Warning, tintColor: Color.Orange };
  return { source: Icon.Info, tintColor: Color.Blue };
}

/** The Doctor's findings; each is fixed in Teitunnel. */
export default function RunDoctor() {
  const { data, isLoading, error, revalidate } = usePromise(
    () => withTeitunnel((client) => client.runDoctor()),
    [],
    { onError: (e) => void explain(e) },
  );
  const fix = () =>
    withTeitunnel((client) => client.open({ view: "doctor" })).catch((e: unknown) => explain(e));

  return (
    <List isLoading={isLoading} isShowingDetail={(data?.length ?? 0) > 0}>
      {error ? (
        <List.EmptyView
          icon={Icon.Plug}
          title="Teitunnel isn't running"
          description={error.message}
          actions={
            <ActionPanel>
              <Action title="Open Teitunnel" icon={Icon.AppWindow} onAction={openTeitunnel} />
            </ActionPanel>
          }
        />
      ) : (
        <List.EmptyView icon={Icon.CheckCircle} title="No problems found" />
      )}
      {sortIssues(data ?? []).map((issue) => (
        <List.Item
          key={issue.id}
          icon={icon(issue)}
          title={issue.title}
          subtitle={issue.subject}
          detail={<List.Item.Detail markdown={`## ${issue.title}\n\n${issue.detail}`} />}
          actions={
            <ActionPanel>
              <Action title="Fix in Teitunnel" icon={Icon.WrenchScrewdriver} onAction={fix} />
              <Action
                title="Check Again"
                icon={Icon.ArrowClockwise}
                shortcut={Keyboard.Shortcut.Common.Refresh}
                onAction={revalidate}
              />
            </ActionPanel>
          }
        />
      ))}
    </List>
  );
}
