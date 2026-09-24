import { Inbox, PowerOff, Send } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { InspectorSection } from "@/components/patterns/inspector";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { FrontView, InboxItem } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useDeliverNow, useFronts, useInboxItems } from "../queries";
import { FrontSheet, type FrontTarget } from "./front-sheet";

function itemState(item: InboxItem): string {
  if (item.deliveredAt !== null) {
    return t("fronts.inbox.delivered", {
      when: relativeTime(item.deliveredAt),
      status: item.status ?? 0,
    });
  }
  if (item.error) return t("fronts.inbox.retrying", { error: item.error });
  return t("fronts.inbox.waiting");
}

/** An inbox's webhooks: when each arrived at Cloudflare and when it was delivered here. */
function InboxActivity({ accountId, inbox }: { accountId: string; inbox: FrontView }) {
  const items = useInboxItems(accountId, inbox.hostname, inbox.path);
  const deliver = useDeliverNow(accountId);
  const waiting = (items.data ?? []).filter((item) => item.deliveredAt === null).length;
  return (
    <div className="flex flex-col gap-2">
      {items.isPending ? (
        <Skeleton className="h-8" />
      ) : items.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(items.error).message}
        </p>
      ) : (items.data ?? []).length === 0 ? (
        <p className="text-callout text-secondary">{t("fronts.inbox.empty")}</p>
      ) : (
        <ol
          aria-label={t("fronts.inbox.activity", { path: inbox.path })}
          className="flex flex-col rounded-card bg-surface-inset px-3 py-0.5"
        >
          {(items.data ?? []).slice(0, 8).map((item) => (
            <li
              key={item.id}
              className="flex min-h-8 flex-col justify-center border-inset border-b-hairline py-1.5 last:border-b-0"
            >
              <span className="truncate font-mono text-callout">
                {item.method} {item.path}
              </span>
              <span className="text-footnote text-secondary tabular">
                {t("fronts.inbox.arrived", { when: relativeTime(item.receivedAt) })} ·{" "}
                {itemState(item)}
              </span>
            </li>
          ))}
        </ol>
      )}
      {waiting > 0 ? (
        <div>
          <Button
            size="sm"
            pending={deliver.isPending}
            onClick={() =>
              deliver.mutate(undefined, {
                onSuccess: (reports) => {
                  const delivered = reports.reduce((sum, r) => sum + r.delivered, 0);
                  toast.success(t("fronts.inbox.deliveredNow", { count: delivered }));
                },
                onError: (error) => toast.error(toIpcError(error).message),
              })
            }
          >
            <Send />
            {t("fronts.inbox.deliverNow")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}

/**
 * A route's Workers on Cloudflare that work while this computer is off: the offline page
 * (instead of error 1033) and webhook inboxes (kept, then delivered in order).
 */
export function FrontsSection({ accountId, hostname }: { accountId: string; hostname: string }) {
  const fronts = useFronts();
  const [editing, setEditing] = useState<FrontTarget | null>(null);
  const mine = (fronts.data ?? []).filter(
    (f) => f.accountId === accountId && f.hostname === hostname.toLowerCase(),
  );
  const offline = mine.find((f) => f.kind === "offline") ?? null;
  const inboxes = mine.filter((f) => f.kind === "inbox");

  return (
    <InspectorSection title={t("fronts.title")}>
      <p className="text-callout text-secondary">{t("fronts.where")}</p>
      {fronts.isPending ? (
        <div className="flex flex-col gap-2" aria-busy>
          <Skeleton className="h-4 w-3/4" />
          <Skeleton className="h-4 w-1/2" />
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-2">
            <PowerOff aria-hidden className="size-3.5 text-secondary" />
            <div className="min-w-0 flex-1">
              <div className="text-body">{t("fronts.offline.label")}</div>
              <div className="truncate text-callout text-secondary">
                {offline?.page ? offline.page.title : t("fronts.off")}
              </div>
            </div>
            <Button
              size="sm"
              onClick={() =>
                setEditing({ kind: "offline", hostname, current: offline?.page ?? null })
              }
            >
              {offline ? t("fronts.edit") : t("fronts.turnOn")}
            </Button>
          </div>
          {inboxes.map((inbox) => (
            <div key={inbox.path} className="flex flex-col gap-2">
              <div className="flex items-center gap-2">
                <Inbox aria-hidden className="size-3.5 text-secondary" />
                <div className="min-w-0 flex-1">
                  <div className="text-body">{t("fronts.inbox.label")}</div>
                  <div className="truncate font-mono text-callout text-secondary">{inbox.path}</div>
                </div>
                <Button
                  size="sm"
                  onClick={() =>
                    setEditing({
                      kind: "inbox",
                      hostname,
                      path: inbox.path,
                      current: inbox.inbox,
                    })
                  }
                >
                  {t("fronts.edit")}
                </Button>
              </div>
              <InboxActivity accountId={accountId} inbox={inbox} />
            </div>
          ))}
          <div>
            <Button
              size="sm"
              onClick={() => setEditing({ kind: "inbox", hostname, path: null, current: null })}
            >
              <Inbox />
              {t("fronts.inbox.add")}
            </Button>
          </div>
        </div>
      )}
      {editing ? (
        <FrontSheet accountId={accountId} target={editing} open onClose={() => setEditing(null)} />
      ) : null}
    </InspectorSection>
  );
}
