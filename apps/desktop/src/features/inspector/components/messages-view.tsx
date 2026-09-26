import { ArrowDownLeft, ArrowUpRight, ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { formatBytes } from "@/features/snapshots/format";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { Direction, StreamStats } from "@/lib/ipc/bindings";
import {
  type DirectionFilter,
  filterMessages,
  formatMs,
  type MessageRow,
  messageRows,
  prettyMessage,
} from "../model";

/** Below this many, the list needs no filter. */
const FILTER_FROM = 6;

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

function Message({ row }: { row: MessageRow }) {
  const [open, setOpen] = useState(false);
  const text = row.text ?? t("inspector.messages.compressed");
  const readable = row.text !== null && row.text !== "";
  return (
    <li>
      <button
        type="button"
        aria-expanded={readable ? open : undefined}
        disabled={!readable}
        onClick={() => setOpen(!open)}
        className="grid w-full grid-cols-[0.75rem_auto_4rem_3.5rem_4rem_minmax(0,1fr)] items-center gap-x-2 px-2 text-left"
      >
        <ChevronRight
          aria-hidden
          className={cn(
            "size-3 text-tertiary transition-transform",
            open && "rotate-90",
            !readable && "invisible",
          )}
        />
        <Arrow direction={row.direction} />
        <span className="text-secondary tabular">{formatMs(row.atUs / 1000)}</span>
        <span className="text-secondary">{row.kind}</span>
        <span className="text-right text-secondary tabular">{formatBytes(row.size)}</span>
        <span className="truncate">
          {text}
          {row.truncated ? "…" : ""}
        </span>
      </button>
      {open && row.text !== null ? (
        <div className="px-2 pt-1 pb-2">
          <CopyField
            value={prettyMessage(row.text)}
            label={t("inspector.messages.copy")}
            multiline
          />
          {row.truncated ? (
            <p className="mt-1 font-sans text-footnote text-secondary">
              {t("inspector.messages.truncated", { size: formatBytes(row.size) })}
            </p>
          ) : null}
        </div>
      ) : null}
    </li>
  );
}

/**
 * WebSocket frames (the most recent, bounded) or event-stream messages, both ways, with
 * time, size and a preview; filter by direction or text, and open one to read it whole
 * (JSON indented) and copy it.
 */
export function MessagesView({ stream }: { stream: StreamStats }) {
  const [direction, setDirection] = useState<DirectionFilter>("all");
  const [query, setQuery] = useState("");
  const rows = useMemo(() => messageRows(stream), [stream]);
  const shown = useMemo(() => filterMessages(rows, direction, query), [rows, direction, query]);
  const frames = stream.frames.length > 0;
  return (
    <div className="flex flex-col gap-1.5">
      <p className="text-callout text-secondary tabular">
        {t("inspector.messages.counts", {
          visitor: stream.client.count,
          service: stream.server.count,
        })}
        {stream.closed ? ` · ${t("inspector.messages.closed")}` : ""}
      </p>
      {rows.length >= FILTER_FROM ? (
        <div className="flex flex-wrap items-center gap-2">
          <SegmentedControl
            label={t("inspector.messages.direction")}
            size="sm"
            segments={[
              { value: "all", label: t("inspector.messages.all") },
              { value: "clientToServer", label: t("inspector.messages.sent") },
              { value: "serverToClient", label: t("inspector.messages.received") },
            ]}
            value={direction}
            onValueChange={setDirection}
          />
          <Input
            type="search"
            aria-label={t("inspector.messages.search")}
            placeholder={t("inspector.messages.search")}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            className="min-w-32 flex-1 rounded-full"
          />
        </div>
      ) : null}
      {rows.length === 0 ? (
        <p className="text-callout text-secondary">{t("inspector.messages.none")}</p>
      ) : shown.length === 0 ? (
        <p className="text-callout text-secondary">{t("inspector.messages.noMatch")}</p>
      ) : (
        <ol
          aria-label={frames ? t("inspector.messages.frames") : t("inspector.messages.label")}
          className="max-h-96 overflow-y-auto rounded-control bg-surface-inset py-1 font-mono text-mono"
        >
          {shown.map((row, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: messages are positional
            <Message key={`${row.atUs}-${index}`} row={row} />
          ))}
        </ol>
      )}
      {stream.framesDropped > 0 ? (
        <p className="text-callout text-secondary">
          {t("inspector.messages.dropped", { count: stream.framesDropped })}
        </p>
      ) : null}
    </div>
  );
}
