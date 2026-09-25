import { CheckCircle2, ExternalLink, MessageSquare, RefreshCw, RotateCcw } from "lucide-react";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Skeleton } from "@/components/ui/skeleton";
import { TextArea } from "@/components/ui/text-area";
import { relativeTime } from "@/lib/format";
import { type MessageKey, t } from "@/lib/i18n";
import type { SubjectKind, SubjectView, Thread_Serialize as Thread } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { useManualRefetch } from "@/lib/use-manual-refetch";
import { useCommentSubjects, useReply, useResolve, useThreads } from "./queries";

type Filter = "open" | "resolved";

const kindLabels: Record<SubjectKind, MessageKey> = {
  quickShare: "comments.kind.quickShare",
  route: "comments.kind.route",
  snapshot: "comments.kind.snapshot",
};

/** Where a thread is on its page, opened in the browser (the overlay jumps to it). */
export function threadUrl(subject: SubjectView, thread: Thread): string | null {
  if (!subject.url) return null;
  const base = subject.url.replace(/\/$/, "");
  return `${base}${thread.path}#__teitunnel-comment=${encodeURIComponent(thread.id)}`;
}

function subtitleOf(subject: SubjectView): string {
  const counts =
    subject.comments === 0
      ? t("comments.none")
      : t("comments.counts", {
          open: t("comments.openCount", { count: subject.open }),
          comments: t("comments.commentCount", { count: subject.comments }),
        });
  return `${t(kindLabels[subject.kind])} · ${counts}`;
}

function ThreadCard({ subject, thread }: { subject: SubjectView; thread: Thread }) {
  const [draft, setDraft] = useState("");
  const reply = useReply(subject.key);
  const resolve = useResolve(subject.key);
  const url = threadUrl(subject, thread);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const body = draft.trim();
    if (!body) return;
    reply.mutate(
      { thread: thread.id, body },
      {
        onSuccess: () => setDraft(""),
        onError: (error) => toast.error(toIpcError(error).message),
      },
    );
  };
  return (
    <article
      aria-label={t("comments.threadLabel", { path: thread.path })}
      className="flex flex-col gap-3 rounded-card bg-surface-inset p-3"
    >
      <header className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate font-mono text-callout text-secondary">
          {thread.path}
        </span>
        {thread.resolved ? (
          <Badge tone="healthy">{t("comments.resolved")}</Badge>
        ) : (
          <Badge tone="accent">{t("comments.open")}</Badge>
        )}
      </header>
      <ol className="flex flex-col gap-2.5">
        {thread.comments.map((comment) => (
          <li key={comment.id} className="flex flex-col gap-0.5">
            <div className="flex flex-wrap items-baseline gap-1.5 text-callout">
              <span className="font-semibold text-primary">{comment.author}</span>
              {comment.byOwner ? <Badge>{t("comments.owner")}</Badge> : null}
              {comment.verified ? (
                <Badge title={comment.email ?? undefined}>{t("comments.signedIn")}</Badge>
              ) : null}
              <span className="text-secondary">{relativeTime(comment.createdAt)}</span>
            </div>
            <p className="selectable whitespace-pre-wrap break-words text-body">{comment.body}</p>
          </li>
        ))}
      </ol>
      <form onSubmit={submit} className="flex flex-col gap-2">
        <TextArea
          aria-label={t("comments.replyLabel")}
          placeholder={t("comments.replyPlaceholder")}
          rows={2}
          maxLength={4000}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) submit(event);
          }}
        />
        <div className="flex flex-wrap items-center gap-2">
          {url ? (
            <Button size="sm" type="button" onClick={() => void openUrl(url)}>
              <ExternalLink />
              {t("comments.openAtSpot")}
            </Button>
          ) : null}
          <Button
            size="sm"
            type="button"
            pending={resolve.isPending}
            onClick={() =>
              resolve.mutate(
                { thread: thread.id, resolved: !thread.resolved },
                { onError: (error) => toast.error(toIpcError(error).message) },
              )
            }
          >
            {thread.resolved ? <RotateCcw /> : <CheckCircle2 />}
            {thread.resolved ? t("comments.reopen") : t("comments.resolve")}
          </Button>
          <Button
            size="sm"
            variant="primary"
            type="submit"
            className="ml-auto"
            disabled={draft.trim() === ""}
            pending={reply.isPending}
          >
            {t("comments.reply")}
          </Button>
        </div>
      </form>
    </article>
  );
}

function Threads({ subject }: { subject: SubjectView }) {
  const threads = useThreads(subject.key);
  const [filter, setFilter] = useState<Filter>("open");
  if (threads.isPending) {
    return (
      <div className="flex flex-col gap-3 p-4" aria-busy>
        <Skeleton className="h-28" />
        <Skeleton className="h-20" />
      </div>
    );
  }
  if (threads.error) {
    const error = toIpcError(threads.error);
    return (
      <ErrorState
        title={t("comments.loadFailed")}
        message={error.message}
        hint={error.hint}
        action={<Button onClick={() => void threads.refetch()}>{t("common.tryAgain")}</Button>}
      />
    );
  }
  const all = threads.data ?? [];
  const shown = all.filter((thread) => thread.resolved === (filter === "resolved"));
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-3 px-4 pt-3 pb-3">
        <div className="min-w-0 flex-1">
          <h2 className="selectable truncate text-title2">{subject.label}</h2>
          <p className="text-callout text-secondary">{t(kindLabels[subject.kind])}</p>
        </div>
        {subject.url ? (
          <Button onClick={() => subject.url && void openUrl(subject.url)}>
            <ExternalLink />
            {t("comments.openPage")}
          </Button>
        ) : null}
      </div>
      <div className="px-4 pb-3">
        <SegmentedControl
          size="sm"
          label={t("comments.filter")}
          segments={[
            {
              value: "open",
              label: t("comments.filterOpen", {
                count: all.filter((thread) => !thread.resolved).length,
              }),
            },
            {
              value: "resolved",
              label: t("comments.filterResolved", {
                count: all.filter((thread) => thread.resolved).length,
              }),
            },
          ]}
          value={filter}
          onValueChange={setFilter}
        />
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 pb-6">
        {shown.length === 0 ? (
          <p className="py-6 text-center text-body text-secondary">
            {filter === "open" ? t("comments.noneOpen") : t("comments.noneResolved")}
          </p>
        ) : (
          shown.map((thread) => <ThreadCard key={thread.id} subject={subject} thread={thread} />)
        )}
      </div>
    </div>
  );
}

/**
 * Comments reviewers pinned to shares, routes and Snapshots: threads per page, reply,
 * resolve and open the page at the spot.
 */
export function CommentsPage({ subject: requested }: { subject?: string | undefined }) {
  const subjects = useCommentSubjects();
  const reload = useManualRefetch(subjects.refetch);
  const [selectedKey, setSelectedKey] = useState<string | null>(requested ?? null);
  const list = subjects.data ?? [];
  const selected = list.find((s) => s.key === selectedKey) ?? list[0] ?? null;

  const toolbar = (
    <TitlebarToolbar title={t("comments.title")}>
      <IconButton
        icon={RefreshCw}
        label={t("comments.refresh")}
        onClick={reload.refresh}
        pending={reload.refreshing}
      />
    </TitlebarToolbar>
  );

  if (subjects.error) {
    const error = toIpcError(subjects.error);
    return (
      <>
        {toolbar}
        <ErrorState
          title={t("comments.loadFailed")}
          message={error.message}
          hint={error.hint}
          action={<Button onClick={() => void subjects.refetch()}>{t("common.tryAgain")}</Button>}
        />
      </>
    );
  }

  if (!subjects.isPending && list.length === 0) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={MessageSquare}
          title={t("comments.empty.title")}
          description={t("comments.empty.description")}
        />
      </>
    );
  }

  return (
    <>
      {toolbar}
      <SplitView
        id="comments"
        list={
          subjects.isPending ? (
            <div className="flex flex-col gap-2 p-3" aria-busy>
              <Skeleton className="h-11" />
              <Skeleton className="h-11" />
            </div>
          ) : (
            <ListPane
              label={t("comments.list")}
              items={list}
              getId={(subject) => subject.key}
              selectedId={selected?.key ?? null}
              onSelect={setSelectedKey}
              renderRow={(subject) => (
                <ListRow
                  title={subject.label}
                  subtitle={subtitleOf(subject)}
                  trailing={
                    subject.unread > 0 ? (
                      <Badge
                        tone="accent"
                        aria-label={t("comments.unread", { count: subject.unread })}
                      >
                        {subject.unread}
                      </Badge>
                    ) : null
                  }
                />
              )}
            />
          )
        }
      >
        {selected ? (
          <Threads key={selected.key} subject={selected} />
        ) : (
          <EmptyState title={t("comments.noSelection")} description="" />
        )}
      </SplitView>
    </>
  );
}
