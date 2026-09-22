import { ArrowDown, Copy, Pause, Play } from "lucide-react";
import { Fragment, useLayoutEffect, useMemo, useRef, useState } from "react";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { cn } from "@/lib/cn";
import type { LogLine } from "@/lib/ipc/bindings";

type Level = "all" | "warn" | "error";

const levels = [
  { value: "all", label: "All" },
  { value: "warn", label: "Warnings" },
  { value: "error", label: "Errors" },
] as const;

const isError = (line: LogLine) => line.level === "error" || line.level === "fatal";

function passes(line: LogLine, level: Level, query: string) {
  if (level === "error" && !isError(line)) return false;
  if (level === "warn" && !isError(line) && line.level !== "warn") return false;
  if (!query) return true;
  const text = `${line.message} ${line.error ?? ""}`.toLowerCase();
  return text.includes(query);
}

/** `text` with every occurrence of `query` marked. */
function Highlight({ text, query }: { text: string; query: string }) {
  if (!query) return <>{text}</>;
  const parts = text.split(new RegExp(`(${query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "i"));
  return (
    <>
      {parts.map((part, index) =>
        index % 2 === 1 ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: split parts are positional
          <mark key={index} className="rounded-[2px] bg-warning/35 text-inherit">
            {part}
          </mark>
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: split parts are positional
          <Fragment key={index}>{part}</Fragment>
        ),
      )}
    </>
  );
}

interface LogViewerProps {
  lines: readonly LogLine[];
  empty: string;
  /** Visible height of the log area. */
  height?: number;
}

/**
 * A connector's log: follows new lines while scrolled to the bottom, stops following
 * when you scroll up (with a pill to jump back), filters by level and text, and can be
 * paused or copied.
 */
export function LogViewer({ lines, empty, height = 224 }: LogViewerProps) {
  const [level, setLevel] = useState<Level>("all");
  const [search, setSearch] = useState("");
  const [paused, setPaused] = useState<readonly LogLine[] | null>(null);
  const [following, setFollowing] = useState(true);
  const scroller = useRef<HTMLDivElement>(null);
  const query = search.trim().toLowerCase();
  const source = paused ?? lines;
  const visible = useMemo(
    () => source.filter((line) => passes(line, level, query)),
    [source, level, query],
  );

  // Keep the newest line in view while following.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-run when lines change
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && following) el.scrollTop = el.scrollHeight;
  }, [visible, following]);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    setFollowing(el.scrollHeight - el.scrollTop - el.clientHeight < 8);
  };

  const copy = () =>
    void navigator.clipboard.writeText(
      visible.map((l) => (l.error ? `${l.message} ${l.error}` : l.message)).join("\n"),
    );

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Input
          type="search"
          aria-label="Search the log"
          placeholder="Search"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          className="min-w-0 flex-1 rounded-full"
        />
        <SegmentedControl
          label="Show"
          size="sm"
          segments={levels}
          value={level}
          onValueChange={setLevel}
        />
        <IconButton
          icon={paused ? Play : Pause}
          label={paused ? "Resume the log" : "Pause the log"}
          size="sm"
          onClick={() => setPaused(paused ? null : lines)}
        />
        <IconButton icon={Copy} label="Copy the visible lines" size="sm" onClick={copy} />
      </div>
      <div className="relative">
        <div
          ref={scroller}
          onScroll={onScroll}
          role="log"
          aria-label="Log"
          aria-live="off"
          style={{ height }}
          className="selectable overflow-y-auto rounded-control bg-surface-inset px-2 py-1.5 font-mono text-[11px] leading-4"
        >
          {visible.length === 0 ? (
            <p className="py-1 font-sans text-callout text-secondary">
              {source.length === 0 ? empty : "No lines match."}
            </p>
          ) : (
            <ol>
              {visible.map((line, index) => (
                <li
                  // biome-ignore lint/suspicious/noArrayIndexKey: log lines are append-only snapshots
                  key={index}
                  className={cn(
                    "break-words",
                    isError(line) && "text-error",
                    line.level === "warn" && "text-warning",
                  )}
                >
                  <Highlight text={line.message} query={query} />
                  {line.error ? (
                    <span className="text-secondary">
                      {" "}
                      <Highlight text={line.error} query={query} />
                    </span>
                  ) : null}
                </li>
              ))}
            </ol>
          )}
        </div>
        {!following && visible.length > 0 ? (
          <button
            type="button"
            onClick={() => setFollowing(true)}
            className="absolute bottom-2 left-1/2 flex h-6 -translate-x-1/2 items-center gap-1 rounded-full bg-accent px-2.5 text-callout text-on-accent shadow-raised"
          >
            <ArrowDown aria-hidden className="size-3" strokeWidth={2.25} /> Jump to Latest
          </button>
        ) : null}
      </div>
      {paused ? (
        <p className="text-callout text-secondary">Paused. New lines appear when you resume.</p>
      ) : null}
    </div>
  );
}
