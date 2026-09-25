import { ArrowDownLeft, ArrowUpRight } from "lucide-react";
import { formatBytes } from "@/features/snapshots/format";
import { t } from "@/lib/i18n";
import type { Direction, StreamStats } from "@/lib/ipc/bindings";
import { formatMs } from "../model";

function Arrow({ direction }: { direction: Direction }) {
  const label =
    direction === "clientToServer"
      ? t("inspector.messages.fromVisitor")
      : t("inspector.messages.fromService");
  const Icon = direction === "clientToServer" ? ArrowUpRight : ArrowDownLeft;
  return (
    <Icon
      aria-label={label}
      className={
        direction === "clientToServer"
          ? "size-3 shrink-0 text-accent"
          : "size-3 shrink-0 text-healthy"
      }
      strokeWidth={2}
    />
  );
}

/**
 * WebSocket frames (the most recent, bounded) or event-stream messages, both ways, with
 * time, size and a preview.
 */
export function MessagesView({ stream }: { stream: StreamStats }) {
  const frames = stream.frames;
  return (
    <div className="flex flex-col gap-1.5">
      <p className="text-callout text-secondary tabular">
        {t("inspector.messages.counts", {
          visitor: stream.client.count,
          service: stream.server.count,
        })}
        {stream.closed ? ` · ${t("inspector.messages.closed")}` : ""}
      </p>
      {frames.length > 0 ? (
        <ol
          aria-label={t("inspector.messages.frames")}
          className="selectable max-h-80 overflow-y-auto rounded-control bg-surface-inset py-1 font-mono text-mono"
        >
          {frames.map((frame, index) => (
            <li
              // biome-ignore lint/suspicious/noArrayIndexKey: frames are positional
              key={index}
              className="grid grid-cols-[auto_4rem_3.5rem_4rem_minmax(0,1fr)] items-center gap-x-2 px-2"
            >
              <Arrow direction={frame.direction} />
              <span className="text-secondary tabular">{formatMs(frame.atUs / 1000)}</span>
              <span className="text-secondary">{frame.opcode}</span>
              <span className="text-right text-secondary tabular">{formatBytes(frame.size)}</span>
              <span className="truncate">
                {frame.closeCode !== null
                  ? `${frame.closeCode} ${frame.closeReason ?? ""}`
                  : (frame.preview ?? t("inspector.messages.compressed"))}
                {frame.truncated ? "…" : ""}
              </span>
            </li>
          ))}
        </ol>
      ) : stream.previews.length > 0 ? (
        <ol
          aria-label={t("inspector.messages.label")}
          className="selectable max-h-80 overflow-y-auto rounded-control bg-surface-inset py-1 font-mono text-mono"
        >
          {stream.previews.map((message, index) => (
            <li
              // biome-ignore lint/suspicious/noArrayIndexKey: messages are positional
              key={index}
              className="grid grid-cols-[auto_4rem_4rem_minmax(0,1fr)] items-start gap-x-2 px-2"
            >
              <Arrow direction={message.direction} />
              <span className="text-secondary tabular">{formatMs(message.atUs / 1000)}</span>
              <span className="text-right text-secondary tabular">{formatBytes(message.size)}</span>
              <span className="break-all whitespace-pre-wrap">
                {message.preview}
                {message.truncated ? "…" : ""}
              </span>
            </li>
          ))}
        </ol>
      ) : (
        <p className="text-callout text-secondary">{t("inspector.messages.none")}</p>
      )}
      {stream.framesDropped > 0 ? (
        <p className="text-callout text-secondary">
          {t("inspector.messages.dropped", { count: stream.framesDropped })}
        </p>
      ) : null}
    </div>
  );
}
