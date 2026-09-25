import { Button } from "@/components/ui/button";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { ExchangeView, HeaderView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { comparable, diffHeaders, diffLines, formatMs } from "../model";
import { useExchange } from "../queries";

interface CompareSheetProps {
  /** The two requests (`null`: closed). */
  pair: readonly [string, string] | null;
  onClose: () => void;
}

/** Two requests side by side: what differs in their summary, headers and bodies. */
export function CompareSheet({ pair, onClose }: CompareSheetProps) {
  const left = useExchange(pair?.[0] ?? null, "compare");
  const right = useExchange(pair?.[1] ?? null, "compare");
  const a = left.data?.view;
  const b = right.data?.view;
  const error = left.error ?? right.error;
  return (
    <Sheet open={pair !== null} onOpenChange={(next) => !next && onClose()}>
      <SheetContent
        title={t("inspector.compare.title")}
        description={
          a && b ? t("inspector.compare.description", { a: a.seq, b: b.seq }) : undefined
        }
        width="lg"
        footer={
          <SheetClose asChild>
            <Button variant="primary">{t("common.done")}</Button>
          </SheetClose>
        }
      >
        {error ? (
          <p role="alert" className="text-callout text-error">
            {toIpcError(error).message}
          </p>
        ) : a && b ? (
          <div className="flex flex-col gap-5">
            <Table
              title={t("inspector.compare.summary")}
              rows={[
                [t("inspector.list.method"), a.request.method, b.request.method],
                [t("inspector.list.path"), pathOf(a), pathOf(b)],
                [
                  t("inspector.list.status"),
                  a.response ? String(a.response.status) : "—",
                  b.response ? String(b.response.status) : "—",
                ],
                [
                  t("inspector.list.duration"),
                  a.durationMs === null ? "—" : formatMs(a.durationMs),
                  b.durationMs === null ? "—" : formatMs(b.durationMs),
                ],
              ]}
            />
            <HeaderTable
              title={t("inspector.compare.requestHeaders")}
              a={a.request.headers}
              b={b.request.headers}
            />
            <BodyDiff
              title={t("inspector.compare.requestBody")}
              a={a.request.body.text}
              b={b.request.body.text}
            />
            <HeaderTable
              title={t("inspector.compare.responseHeaders")}
              a={a.response?.headers ?? []}
              b={b.response?.headers ?? []}
            />
            <BodyDiff
              title={t("inspector.compare.responseBody")}
              a={a.response?.body.text ?? null}
              b={b.response?.body.text ?? null}
            />
          </div>
        ) : (
          <div className="flex flex-col gap-2" aria-busy>
            <Skeleton className="h-4 w-2/3" />
            <Skeleton className="h-24 w-full" />
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}

const pathOf = (view: ExchangeView) =>
  view.request.query ? `${view.request.path}?${view.request.query}` : view.request.path;

function Table({ title, rows }: { title: string; rows: [string, string, string][] }) {
  return (
    <section className="flex flex-col gap-1.5">
      <h3 className="text-headline text-secondary">{title}</h3>
      <table className="w-full table-fixed text-callout">
        <tbody>
          {rows.map(([label, left, right]) => (
            <tr
              key={label}
              className={cn(left !== right && "bg-warning/10")}
              aria-label={left === right ? undefined : t("inspector.compare.differs", { label })}
            >
              <th
                scope="row"
                className="w-28 truncate py-0.5 pr-2 text-left font-normal text-secondary"
              >
                {label}
              </th>
              <td className="selectable truncate py-0.5 pr-2 font-mono text-mono">{left}</td>
              <td className="selectable truncate py-0.5 font-mono text-mono">{right}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function HeaderTable({
  title,
  a,
  b,
}: {
  title: string;
  a: readonly HeaderView[];
  b: readonly HeaderView[];
}) {
  const rows = diffHeaders(a, b);
  const changed = rows.filter((row) => row.left !== row.right);
  return (
    <section className="flex flex-col gap-1.5">
      <h3 className="text-headline text-secondary">
        {title}{" "}
        <span className="font-normal">
          · {t("inspector.compare.changed", { count: changed.length })}
        </span>
      </h3>
      {rows.length === 0 ? (
        <p className="text-callout text-secondary">{t("inspector.detail.noHeaders")}</p>
      ) : (
        <table className="w-full table-fixed font-mono text-mono">
          <tbody>
            {rows.map((row) => {
              const differs = row.left !== row.right;
              return (
                <tr key={row.name} className={cn(differs && "bg-warning/10")}>
                  <th
                    scope="row"
                    className="w-40 truncate py-0.5 pr-2 text-left font-normal text-secondary"
                    title={row.name}
                  >
                    {row.name}
                  </th>
                  <td
                    className={cn(
                      "selectable py-0.5 pr-2 break-all align-top",
                      row.left === null && "text-tertiary",
                    )}
                  >
                    {row.left ?? "—"}
                  </td>
                  <td
                    className={cn(
                      "selectable py-0.5 break-all align-top",
                      row.right === null && "text-tertiary",
                    )}
                  >
                    {row.right ?? "—"}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}

function BodyDiff({ title, a, b }: { title: string; a: string | null; b: string | null }) {
  const left = comparable(a);
  const right = comparable(b);
  const lines = left === right ? null : diffLines(left, right);
  return (
    <section className="flex flex-col gap-1.5">
      <h3 className="text-headline text-secondary">{title}</h3>
      {left === right ? (
        <p className="text-callout text-secondary">
          {left === "" ? t("inspector.body.none") : t("inspector.compare.same")}
        </p>
      ) : lines === null ? (
        <p className="text-callout text-secondary">{t("inspector.compare.tooLarge")}</p>
      ) : (
        <pre className="selectable max-h-80 overflow-auto rounded-control bg-surface-inset py-1.5 font-mono text-mono">
          {lines.map((line, index) => (
            <div
              // biome-ignore lint/suspicious/noArrayIndexKey: diff lines are positional
              key={index}
              className={cn(
                "px-2 whitespace-pre-wrap break-all",
                line.kind === "added" && "bg-healthy/15",
                line.kind === "removed" && "bg-error/15",
              )}
            >
              <span aria-hidden className="mr-2 inline-block w-2 text-secondary">
                {line.kind === "added" ? "+" : line.kind === "removed" ? "−" : " "}
              </span>
              <span className="sr-only">
                {line.kind === "added"
                  ? t("inspector.compare.added")
                  : line.kind === "removed"
                    ? t("inspector.compare.removed")
                    : ""}
              </span>
              {line.text}
            </div>
          ))}
        </pre>
      )}
    </section>
  );
}
