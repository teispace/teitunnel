import { Eye, EyeOff, FileOutput, Pencil, Repeat } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip } from "@/components/ui/tooltip";
import { cn } from "@/lib/cn";
import { type MessageKey, t } from "@/lib/i18n";
import type { ExchangeDetail as Detail, ExchangeRow, ExchangeView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { formatMs, statusTone, toneText } from "../model";
import { revealExchange, useExchange, useReplay } from "../queries";
import { BodyView } from "./body-view";
import { HoldLikeThis } from "./breakpoint-rules";
import { HeadersView } from "./headers-view";
import { MessagesView } from "./messages-view";
import { TimingBar } from "./timing-bar";
import { WebhookPanel } from "./webhook-panel";

type Part = "request" | "response" | "messages";

/** Who answered, in words. */
function responder(view: ExchangeView): string {
  const who = view.responder;
  switch (who.type) {
    case "upstream":
      return t("inspector.responder.upstream");
    case "folder":
      return t("inspector.responder.folder");
    case "stub":
      return who.fallback ? t("inspector.responder.stubFallback") : t("inspector.responder.stub");
    case "gate":
      return t("inspector.responder.gate", {
        reason: t(`inspector.gate.${who.reason}` as MessageKey),
      });
    case "paused":
      return t("inspector.responder.paused");
    case "fault":
      return t("inspector.responder.fault");
    case "breakpoint":
      return t("inspector.responder.breakpoint");
    case "lens":
      return t("inspector.responder.lens");
  }
}

interface ExchangeDetailProps {
  row: ExchangeRow;
  /** The share or route that captured it. */
  tapName: string | null;
  /** Replays were sent (newest last). */
  onReplayed: (rows: ExchangeRow[]) => void;
  onEdit: (detail: Detail) => void;
  onExport: () => void;
}

/**
 * One request: summary, timing, webhook signature, then request, response and stream
 * messages. Masked by default; Show Secrets reads it once more with credentials, kept
 * only here until hidden or another request is selected.
 */
export function ExchangeDetail({
  row,
  tapName,
  onReplayed,
  onEdit,
  onExport,
}: ExchangeDetailProps) {
  const version = `${row.state}:${row.status ?? ""}:${row.durationMs ?? ""}:${row.responseBytes ?? ""}`;
  const query = useExchange(row.id, version);
  const replay = useReplay();
  const [revealed, setRevealed] = useState<Detail | null>(null);
  const [revealing, setRevealing] = useState(false);
  const [revealError, setRevealError] = useState<string | null>(null);
  const [part, setPart] = useState<Part>("request");

  // Secrets belong to the request they were shown for.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset when the request changes
  useEffect(() => {
    setRevealed(null);
    setRevealError(null);
    replay.reset();
  }, [row.id]);

  const masked = query.data?.view.id === row.id ? query.data : undefined;
  const detail = revealed ?? masked;

  const reveal = () => {
    if (revealed) {
      setRevealed(null);
      return;
    }
    setRevealing(true);
    setRevealError(null);
    revealExchange(row.id)
      .then(setRevealed)
      .catch((error: unknown) => setRevealError(toIpcError(error).message))
      .finally(() => setRevealing(false));
  };

  const sendAgain = () =>
    replay.mutate(
      { id: row.id, input: {} },
      {
        onSuccess: (rows) => {
          toast.success(t("inspector.replay.sent", { count: rows.length }));
          onReplayed(rows);
        },
        onError: (error) => toast.error(toIpcError(error).message),
      },
    );

  if (!detail) {
    return (
      <div className="flex flex-col gap-3 px-4 pt-4" aria-busy>
        {query.error ? (
          <p role="alert" className="text-callout text-error">
            {toIpcError(query.error).message}
          </p>
        ) : (
          <>
            <Skeleton className="h-5 w-3/4" />
            <Skeleton className="h-4 w-1/2" />
            <Skeleton className="h-24 w-full" />
          </>
        )}
      </div>
    );
  }

  const view = detail.view;
  const stream = view.stream;
  const parts: Part[] = [
    "request",
    "response",
    ...(stream && (stream.frames.length > 0 || stream.previews.length > 0 || view.kind !== "http")
      ? (["messages"] as const)
      : []),
  ];
  const shown = parts.includes(part) ? part : "request";
  const tone = statusTone(view.response?.status ?? null, view.state === "failed");
  const revealLabel = revealed ? t("inspector.detail.hide") : t("inspector.detail.reveal");

  return (
    <section
      aria-label={t("inspector.detail.label")}
      className="flex min-h-0 min-w-0 flex-1 flex-col"
    >
      <div className="flex flex-col gap-2 border-separator border-b-hairline px-4 pt-3 pb-3">
        <h2 className="selectable line-clamp-2 break-all font-mono text-[13px] leading-5 font-semibold">
          {view.request.method} {view.request.path}
          {view.request.query ? `?${view.request.query}` : ""}
        </h2>
        <div className="flex flex-wrap items-center gap-x-2 text-callout text-secondary">
          {view.response ? (
            <span className={cn("font-medium tabular", toneText[tone])}>
              {view.response.status} {view.response.statusText}
            </span>
          ) : (
            <span>
              {view.state === "failed"
                ? t("inspector.list.failed")
                : t(`inspector.state.${view.state}` as MessageKey)}
            </span>
          )}
          {view.durationMs !== null ? (
            <span className="tabular">· {formatMs(view.durationMs)}</span>
          ) : null}
          <span className="truncate">· {view.request.host}</span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" pending={replay.isPending} onClick={sendAgain}>
            <Repeat /> {t("inspector.detail.replay")}
          </Button>
          <Button size="sm" onClick={() => onEdit(detail)}>
            <Pencil /> {t("inspector.detail.edit")}
          </Button>
          <Button size="sm" onClick={onExport}>
            <FileOutput /> {t("inspector.detail.export")}
          </Button>
          <HoldLikeThis tap={row.tap} method={view.request.method} path={view.request.path} />
          <Tooltip
            content={
              detail.restored ? t("inspector.detail.restored") : t("inspector.detail.revealHelp")
            }
          >
            <IconButton
              icon={revealed ? EyeOff : Eye}
              label={revealLabel}
              variant="secondary"
              className="ml-auto"
              aria-pressed={revealed !== null}
              pending={revealing}
              disabled={detail.restored && !revealed}
              onClick={reveal}
            />
          </Tooltip>
        </div>
        {revealed ? (
          <p role="status" className="text-callout text-warning">
            {t("inspector.detail.revealed")}
          </p>
        ) : null}
        {revealError ? (
          <p role="alert" className="text-callout text-error">
            {revealError}
          </p>
        ) : null}
        {detail.restored ? (
          <p className="text-callout text-secondary">{t("inspector.detail.restored")}</p>
        ) : null}
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-4 pt-3 pb-6">
        <InspectorSection title={t("inspector.detail.summary")}>
          <KeyValueGrid
            items={[
              { label: t("inspector.detail.url"), value: view.request.url, mono: true },
              ...(tapName ? [{ label: t("inspector.detail.tap"), value: tapName }] : []),
              { label: t("inspector.detail.answeredBy"), value: responder(view) },
              {
                label: t("inspector.detail.visitor"),
                value: view.client.country
                  ? `${view.client.ip} (${view.client.country})`
                  : view.client.ip,
                mono: true,
              },
              { label: t("inspector.detail.protocol"), value: view.request.httpVersion },
              ...(view.client.cfRay
                ? [{ label: t("inspector.detail.ray"), value: view.client.cfRay, mono: true }]
                : []),
              ...(view.error
                ? [
                    {
                      label: t("inspector.detail.error"),
                      value: `${t(`inspector.errorKind.${view.error.kind}` as MessageKey)} · ${view.error.message}`,
                    },
                  ]
                : []),
              ...(view.breakpoint &&
              (view.breakpoint.requestEdited ||
                view.breakpoint.responseEdited ||
                view.breakpoint.timedOut)
                ? [
                    {
                      label: t("inspector.detail.breakpoint"),
                      value: view.breakpoint.timedOut
                        ? t("inspector.held.timedOut")
                        : t("inspector.held.edited"),
                    },
                  ]
                : []),
              ...(view.fault
                ? [{ label: t("inspector.detail.fault"), value: faultText(view) }]
                : []),
            ]}
          />
        </InspectorSection>
        <InspectorSection title={t("inspector.timing.title")}>
          <TimingBar timings={view.timings} durationMs={view.durationMs} />
        </InspectorSection>
        {detail.webhook ? (
          <InspectorSection title={t("inspector.webhook.title")}>
            <WebhookPanel check={detail.webhook} tap={view.tap} />
          </InspectorSection>
        ) : null}
        <div className="flex flex-col gap-3">
          <SegmentedControl
            label={t("inspector.detail.part")}
            segments={parts.map((value) => ({
              value,
              label: t(`inspector.detail.${value}` as MessageKey),
            }))}
            value={shown}
            onValueChange={setPart}
            className="self-start"
          />
          {shown === "request" ? (
            <>
              <InspectorSection title={t("inspector.detail.headers")}>
                <HeadersView headers={view.request.headers} />
              </InspectorSection>
              <InspectorSection title={t("inspector.detail.body")}>
                <BodyView body={view.request.body} label={t("inspector.detail.request")} />
              </InspectorSection>
            </>
          ) : shown === "response" ? (
            view.response ? (
              <>
                <InspectorSection title={t("inspector.detail.headers")}>
                  <HeadersView headers={view.response.headers} />
                </InspectorSection>
                <InspectorSection title={t("inspector.detail.body")}>
                  <BodyView body={view.response.body} label={t("inspector.detail.response")} />
                </InspectorSection>
              </>
            ) : (
              <p className="text-callout text-secondary">{t("inspector.detail.noResponse")}</p>
            )
          ) : stream ? (
            <MessagesView stream={stream} />
          ) : null}
        </div>
      </div>
    </section>
  );
}

function faultText(view: ExchangeView): string {
  const action = view.fault?.action;
  if (!action) return "";
  switch (action.type) {
    case "status":
      return t("inspector.fault.status", { status: action.status });
    case "reset":
      return t("inspector.fault.reset");
    case "delay":
      return t("inspector.fault.delay", { duration: formatMs(action.ms) });
    case "timeout":
      return t("inspector.fault.timeout", { duration: formatMs(action.after_ms) });
  }
}
