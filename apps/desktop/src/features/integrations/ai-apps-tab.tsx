import { useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { AiClientCheck, AiClientState, AiClientView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import {
  useAiClients,
  useRevealAiClient,
  useSetAiClientConnected,
  useTestAiClient,
} from "./queries";

/** One line on where the app stands, and when it was last used. */
export function describeClient(client: AiClientView): string {
  const state = stateText(client.state, client.problem);
  if (client.state !== "connected" && client.state !== "needsUpdate") return state;
  const used =
    client.lastUsedAt === null
      ? t("ai.apps.neverUsed")
      : t("ai.apps.lastUsed", { when: relativeTime(client.lastUsedAt) });
  return `${state} · ${used}`;
}

function stateText(state: AiClientState, problem: string | null): string {
  switch (state) {
    case "connected":
      return t("ai.apps.state.connected");
    case "needsUpdate":
      return t("ai.apps.state.needsUpdate");
    case "leftover":
      return t("ai.apps.state.leftover");
    case "notConnected":
      return t("ai.apps.state.notConnected");
    case "unreadable":
      return t("ai.apps.state.unreadable", { problem: problem ?? "" });
    case "notInstalled":
      return t("ai.apps.state.notInstalled");
  }
}

type Check = { ok: true; result: AiClientCheck } | { ok: false; message: string };

function AppRow({ client, canConnect }: { client: AiClientView; canConnect: boolean }) {
  const change = useSetAiClientConnected();
  const test = useTestAiClient();
  const reveal = useRevealAiClient();
  const [check, setCheck] = useState<Check | null>(null);
  const connected = client.state === "connected" || client.state === "needsUpdate";
  const run = async () => {
    setCheck(null);
    try {
      setCheck({ ok: true, result: await test.mutateAsync(client.id) });
    } catch (error) {
      setCheck({ ok: false, message: toIpcError(error).message });
    }
  };
  const primary =
    client.state === "needsUpdate" ? (
      <Button
        size="sm"
        variant="primary"
        pending={change.isPending && change.variables?.connect === true}
        disabled={change.isPending || !canConnect}
        onClick={() => change.mutate({ id: client.id, connect: true })}
      >
        {t("ai.apps.update")}
      </Button>
    ) : client.state === "notConnected" ? (
      <Button
        size="sm"
        variant="primary"
        pending={change.isPending}
        disabled={change.isPending || !canConnect}
        onClick={() => change.mutate({ id: client.id, connect: true })}
      >
        {t("ai.apps.connect")}
      </Button>
    ) : null;
  const remove =
    connected || client.state === "leftover" ? (
      <Button
        size="sm"
        pending={change.isPending && change.variables?.connect === false}
        disabled={change.isPending}
        onClick={() => change.mutate({ id: client.id, connect: false })}
      >
        {client.state === "leftover" ? t("ai.apps.remove") : t("ai.apps.disconnect")}
      </Button>
    ) : null;
  return (
    <div className="flex flex-col py-1">
      <GroupedRow label={client.name} description={describeClient(client)}>
        {connected ? (
          <Button size="sm" pending={test.isPending} onClick={() => void run()}>
            {test.isPending ? t("ai.apps.testing") : t("ai.apps.test")}
          </Button>
        ) : null}
        {remove}
        {primary}
      </GroupedRow>
      {check ? (
        <p
          role={check.ok ? "status" : "alert"}
          className={check.ok ? "pb-2 text-callout text-healthy" : "pb-2 text-callout text-error"}
        >
          {check.ok
            ? t("ai.apps.works", {
                server: check.result.server,
                count: check.result.tools,
                ms: check.result.millis,
              })
            : t("ai.apps.failed", { message: check.message })}
        </p>
      ) : null}
      {change.error && change.variables?.id === client.id ? (
        <p role="alert" className="pb-2 text-callout text-error">
          {toIpcError(change.error).message}
        </p>
      ) : null}
      <Disclosure title={t("ai.apps.details")} className="pb-1.5">
        <dl className="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1.5 pt-1 text-callout">
          <dt className="text-secondary">{t("ai.apps.configFile")}</dt>
          <dd className="flex min-w-0 items-center gap-2">
            <CopyField value={client.path} label={t("ai.apps.configFile")} className="flex-1" />
            <Button size="sm" onClick={() => reveal.mutate(client.id)}>
              {t("ai.apps.reveal")}
            </Button>
          </dd>
          {client.command ? (
            <>
              <dt className="text-secondary">{t("ai.apps.runs")}</dt>
              <dd className="min-w-0">
                <CopyField value={client.command} label={t("ai.apps.runs")} />
              </dd>
            </>
          ) : null}
          {client.installedAt ? (
            <>
              <dt className="text-secondary">{t("ai.apps.foundAt")}</dt>
              <dd className="selectable min-w-0 truncate font-mono text-mono">
                {client.installedAt}
              </dd>
            </>
          ) : null}
        </dl>
      </Disclosure>
    </div>
  );
}

/**
 * AI & Integrations ▸ AI Apps: connect the AI apps found on this computer to
 * Teitunnel's MCP server, check that they really start it, and see when each last
 * did. Apps that aren't installed are listed apart, with their settings to add by hand.
 */
export function AiAppsTab() {
  const { data } = useAiClients();
  if (!data) return <SkeletonSection rows={4} />;
  const present = data.clients.filter((c) => c.state !== "notInstalled");
  const absent = data.clients.filter((c) => c.state === "notInstalled");
  const canConnect = data.command !== null;
  return (
    <>
      <p className="px-2.5 text-callout text-secondary">{t("ai.apps.intro")}</p>
      {canConnect ? null : (
        <p role="alert" className="px-2.5 text-callout text-warning">
          {t("ai.apps.noCli")}
        </p>
      )}
      <GroupedSection title={t("ai.apps.installed")} footer={t("ai.apps.installedFooter")}>
        {present.length === 0 ? (
          <p className="py-2 text-callout text-secondary">{t("ai.apps.none")}</p>
        ) : (
          present.map((client) => (
            <AppRow key={client.id} client={client} canConnect={canConnect} />
          ))
        )}
      </GroupedSection>
      {data.command ? (
        <GroupedSection>
          <div className="flex flex-col gap-1.5 py-2">
            <p className="text-callout text-secondary">{t("ai.apps.anyClient")}</p>
            <CopyField label={t("ai.apps.command")} value={`${data.command} mcp`} />
          </div>
        </GroupedSection>
      ) : null}
      {absent.length > 0 ? (
        <Disclosure title={t("ai.apps.others", { count: absent.length })} className="px-2.5">
          <div className="flex flex-col gap-3 pt-2">
            <p className="text-callout text-secondary">{t("ai.apps.othersDetail")}</p>
            {absent.map((client) =>
              client.snippet ? (
                <div key={client.id} className="flex flex-col gap-1">
                  <span className="text-callout">
                    {client.name}{" "}
                    <span className="font-mono text-secondary text-mono">{client.path}</span>
                  </span>
                  <CopyField
                    multiline
                    label={t("ai.apps.copySettings", { name: client.name })}
                    value={client.snippet}
                  />
                </div>
              ) : null,
            )}
          </div>
        </Disclosure>
      ) : null}
    </>
  );
}
