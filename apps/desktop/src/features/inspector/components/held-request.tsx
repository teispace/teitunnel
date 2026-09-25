import { OctagonPause } from "lucide-react";
import { type KeyboardEvent, type ReactNode, useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { TextArea } from "@/components/ui/text-area";
import { t } from "@/lib/i18n";
import type { ExchangeRow, Paused, Resume } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useNow } from "@/lib/use-now";
import {
  type AnswerDraft,
  answerResume,
  type HeldDraft,
  heldChanged,
  heldDraft,
  heldResume,
  newAnswer,
  secondsLeft,
} from "../model";
import { heldExchange, useResume } from "../queries";

function Field({
  label,
  help,
  children,
}: {
  label: string;
  help?: string;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-callout text-secondary">
        {label}
      </label>
      {children(id)}
      {help ? <p className="text-footnote text-tertiary">{help}</p> : null}
    </div>
  );
}

/**
 * A request held at a breakpoint: what goes on, editable, with a countdown to when it
 * goes on by itself. Continue (with changes), answer it yourself (before it reaches the
 * service) or drop it. ⌘↩ continues.
 */
export function HeldRequest({ row }: { row: ExchangeRow }) {
  const [held, setHeld] = useState<Paused | null | undefined>(undefined);
  const [draft, setDraft] = useState<HeldDraft | null>(null);
  const [answer, setAnswer] = useState<AnswerDraft | null>(null);
  const resume = useResume();
  const now = useNow();

  // Read it again whenever it stops somewhere new (the request, then the answer).
  // biome-ignore lint/correctness/useExhaustiveDependencies: once per stop
  useEffect(() => {
    let current = true;
    setHeld(undefined);
    setAnswer(null);
    resume.reset();
    heldExchange(row.id)
      .then((found) => {
        if (!current) return;
        setHeld(found);
        setDraft(found ? heldDraft(found) : null);
      })
      .catch(() => current && setHeld(null));
    return () => {
      current = false;
    };
  }, [row.id, row.paused]);

  if (held === undefined) {
    return (
      <div className="flex flex-col gap-3 px-4 pt-4" aria-busy>
        <Skeleton className="h-5 w-2/3" />
        <Skeleton className="h-24 w-full" />
      </div>
    );
  }
  if (held === null || !draft) {
    return <p className="m-auto px-4 text-callout text-secondary">{t("inspector.held.gone")}</p>;
  }

  const go = (how: Resume) => resume.mutate({ id: held.exchange, resume: how });
  const changed = heldChanged(held, draft);
  const set = (next: Partial<HeldDraft>) => setDraft({ ...draft, ...next });
  const atRequest = held.stage === "request";
  const lock = held.bodyLocked;
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey)) return;
    event.preventDefault();
    go(answer ? answerResume(answer) : heldResume(held, draft));
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: ⌘↩ anywhere in the editor
    <div className="flex min-h-0 flex-1 flex-col" onKeyDown={onKeyDown}>
      <div className="flex flex-col gap-1 border-separator border-b-hairline px-4 pt-3 pb-3">
        <h2 className="flex items-center gap-2 text-headline">
          <OctagonPause aria-hidden className="size-4 shrink-0 text-accent" strokeWidth={2} />
          {atRequest ? t("inspector.held.request") : t("inspector.held.response")}
        </h2>
        <p className="selectable truncate font-mono text-mono text-secondary">
          {held.method} {held.host}
          {held.target}
        </p>
        <p className="text-callout text-secondary tabular" aria-live="off">
          {t("inspector.held.resumesIn", { seconds: secondsLeft(held, now) })}
        </p>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pt-3 pb-4">
        {answer ? (
          <>
            <h3 className="text-headline">{t("inspector.held.answerTitle")}</h3>
            <Field label={t("inspector.held.status")}>
              {(id) => (
                <Input
                  id={id}
                  inputMode="numeric"
                  className="w-20 tabular"
                  value={answer.status}
                  onChange={(e) =>
                    setAnswer({ ...answer, status: e.target.value.replace(/\D/g, "").slice(0, 3) })
                  }
                />
              )}
            </Field>
            <Field label={t("inspector.held.headers")} help={t("inspector.held.headersHelp")}>
              {(id) => (
                <TextArea
                  id={id}
                  rows={3}
                  spellCheck={false}
                  className="font-mono text-mono"
                  value={answer.headers}
                  onChange={(e) => setAnswer({ ...answer, headers: e.target.value })}
                />
              )}
            </Field>
            <Field label={t("inspector.held.body")}>
              {(id) => (
                <TextArea
                  id={id}
                  rows={8}
                  spellCheck={false}
                  className="font-mono text-mono"
                  value={answer.body}
                  onChange={(e) => setAnswer({ ...answer, body: e.target.value })}
                />
              )}
            </Field>
          </>
        ) : (
          <>
            {atRequest ? (
              <div className="flex gap-2">
                <Field label={t("inspector.held.method")}>
                  {(id) => (
                    <Input
                      id={id}
                      className="w-24 font-mono text-mono uppercase"
                      value={draft.method}
                      onChange={(e) => set({ method: e.target.value })}
                    />
                  )}
                </Field>
                <div className="min-w-0 flex-1">
                  <Field label={t("inspector.held.target")}>
                    {(id) => (
                      <Input
                        id={id}
                        className="font-mono text-mono"
                        value={draft.target}
                        onChange={(e) => set({ target: e.target.value })}
                      />
                    )}
                  </Field>
                </div>
              </div>
            ) : (
              <Field label={t("inspector.held.status")}>
                {(id) => (
                  <Input
                    id={id}
                    inputMode="numeric"
                    className="w-20 tabular"
                    value={draft.status}
                    onChange={(e) => set({ status: e.target.value.replace(/\D/g, "").slice(0, 3) })}
                  />
                )}
              </Field>
            )}
            <Field label={t("inspector.held.headers")} help={t("inspector.held.headersHelp")}>
              {(id) => (
                <TextArea
                  id={id}
                  rows={Math.min(12, Math.max(3, held.headers.length + 2))}
                  spellCheck={false}
                  className="font-mono text-mono"
                  value={draft.headers}
                  onChange={(e) => set({ headers: e.target.value })}
                />
              )}
            </Field>
            {lock ? (
              <Field label={t("inspector.held.body")}>
                {() => (
                  <p className="text-callout text-secondary">
                    {t(`inspector.held.locked.${lock}`)}
                  </p>
                )}
              </Field>
            ) : (
              <Field label={t("inspector.held.body")}>
                {(id) => (
                  <TextArea
                    id={id}
                    rows={8}
                    spellCheck={false}
                    className="font-mono text-mono"
                    value={draft.body}
                    onChange={(e) => set({ body: e.target.value })}
                  />
                )}
              </Field>
            )}
          </>
        )}
        {resume.error ? (
          <p role="alert" className="text-callout text-error">
            {toIpcError(resume.error).message}
          </p>
        ) : null}
      </div>
      <div className="flex flex-wrap items-center gap-2 border-separator border-t-hairline px-4 py-3">
        <Button
          variant="destructive"
          size="sm"
          title={t("inspector.held.dropHelp")}
          disabled={resume.isPending}
          onClick={() => go({ type: "abort" })}
        >
          {t("inspector.held.drop")}
        </Button>
        <div className="ml-auto flex items-center gap-2">
          {answer ? (
            <>
              <Button size="sm" disabled={resume.isPending} onClick={() => setAnswer(null)}>
                {t("inspector.held.backToRequest")}
              </Button>
              <Button
                variant="primary"
                size="sm"
                pending={resume.isPending}
                onClick={() => go(answerResume(answer))}
              >
                {t("inspector.held.sendAnswer")}
              </Button>
            </>
          ) : (
            <>
              {atRequest ? (
                <Button
                  size="sm"
                  disabled={resume.isPending}
                  onClick={() => setAnswer(newAnswer())}
                >
                  {t("inspector.held.answer")}
                </Button>
              ) : null}
              {changed ? (
                <Button
                  size="sm"
                  disabled={resume.isPending}
                  onClick={() => setDraft(heldDraft(held))}
                >
                  {t("inspector.held.revert")}
                </Button>
              ) : null}
              <Button
                variant="primary"
                size="sm"
                pending={resume.isPending}
                onClick={() => go(heldResume(held, draft))}
              >
                {changed ? t("inspector.held.continueChanged") : t("inspector.held.continue")}
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
