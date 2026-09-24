import { type FormEvent, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { TextArea } from "@/components/ui/text-area";
import { t } from "@/lib/i18n";
import type { ExchangeDetail, ExchangeRow } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { headersText, providerNames, type ReplayDraft, replayInput } from "../model";
import { useReplay } from "../queries";

interface ReplaySheetProps {
  /** The request to send again (`null`: closed). */
  detail: ExchangeDetail | null;
  onClose: () => void;
  onReplayed: (rows: ExchangeRow[]) => void;
}

function draftOf(detail: ExchangeDetail): ReplayDraft {
  const request = detail.view.request;
  return {
    method: request.method,
    path: request.query ? `${request.path}?${request.query}` : request.path,
    headers: headersText(request.headers),
    body: request.body.text ?? "",
    times: 1,
    resign: false,
  };
}

/**
 * Edit and replay: method, path, headers and body, sent up to 100 times, optionally
 * re-signed with the saved webhook secret. Only what changed is sent, so a masked header
 * left alone keeps its real value.
 */
export function ReplaySheet({ detail, onClose, onReplayed }: ReplaySheetProps) {
  const [draft, setDraft] = useState<ReplayDraft | null>(null);
  const [times, setTimes] = useState("1");
  const replay = useReplay();

  // Start from the captured request each time the sheet opens.
  // biome-ignore lint/correctness/useExhaustiveDependencies: once per opening
  useEffect(() => {
    if (!detail) return;
    setDraft(draftOf(detail));
    setTimes("1");
    replay.reset();
  }, [detail?.view.id]);

  const open = detail !== null && draft !== null;
  const request = detail?.view.request;
  const binary = request ? request.body.text === null && request.body.base64 !== null : false;
  const webhook = detail?.webhook ?? null;
  const count = Math.min(100, Math.max(1, Number.parseInt(times, 10) || 1));

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!detail || !draft || !request) return;
    const input = replayInput(
      {
        method: request.method,
        path: draftOf(detail).path,
        headers: request.headers,
        body: binary ? "" : request.body.text,
      },
      { ...draft, times: count, body: binary ? "" : draft.body },
    );
    replay.mutate(
      { id: detail.view.id, input },
      {
        onSuccess: (rows) => {
          toast.success(t("inspector.replay.sent", { count: rows.length }));
          onReplayed(rows);
          onClose();
        },
      },
    );
  };

  return (
    <Sheet open={open} onOpenChange={(next) => !next && !replay.isPending && onClose()}>
      <SheetContent
        title={t("inspector.replay.title")}
        description={t("inspector.replay.description")}
        width="lg"
        footer={
          <>
            <SheetClose asChild>
              <Button disabled={replay.isPending}>{t("common.cancel")}</Button>
            </SheetClose>
            <Button variant="primary" type="submit" form="replay-form" pending={replay.isPending}>
              {count > 1 ? t("inspector.replay.sendTimes", { count }) : t("inspector.replay.send")}
            </Button>
          </>
        }
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        {draft ? (
          <form id="replay-form" onSubmit={submit} className="flex flex-col gap-4">
            <div className="grid grid-cols-[7rem_minmax(0,1fr)] gap-2">
              <Field label={t("inspector.replay.method")}>
                {(control) => (
                  <Input
                    {...control}
                    className="font-mono text-mono uppercase"
                    value={draft.method}
                    onChange={(event) => setDraft({ ...draft, method: event.target.value })}
                  />
                )}
              </Field>
              <Field label={t("inspector.replay.path")}>
                {(control) => (
                  <Input
                    {...control}
                    className="font-mono text-mono"
                    value={draft.path}
                    onChange={(event) => setDraft({ ...draft, path: event.target.value })}
                  />
                )}
              </Field>
            </div>
            <Field label={t("inspector.replay.headers")} help={t("inspector.replay.headersHelp")}>
              {(control) => (
                <TextArea
                  {...control}
                  rows={8}
                  className="font-mono text-mono whitespace-pre"
                  wrap="off"
                  value={draft.headers}
                  onChange={(event) => setDraft({ ...draft, headers: event.target.value })}
                />
              )}
            </Field>
            {binary ? (
              <p className="text-callout text-secondary">{t("inspector.replay.binaryBody")}</p>
            ) : (
              <Field label={t("inspector.replay.body")}>
                {(control) => (
                  <TextArea
                    {...control}
                    rows={8}
                    className="font-mono text-mono"
                    value={draft.body}
                    onChange={(event) => setDraft({ ...draft, body: event.target.value })}
                  />
                )}
              </Field>
            )}
            <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
              <label htmlFor="replay-times" className="flex items-center gap-2 text-body">
                {t("inspector.replay.times")}
                <Input
                  id="replay-times"
                  inputMode="numeric"
                  className="w-16 tabular"
                  value={times}
                  onChange={(event) => setTimes(event.target.value.replace(/\D/g, "").slice(0, 3))}
                />
              </label>
              <label htmlFor="replay-resign" className="flex items-center gap-2 text-body">
                <Checkbox
                  id="replay-resign"
                  checked={draft.resign}
                  disabled={!webhook?.hasSecret}
                  onCheckedChange={(on) => setDraft({ ...draft, resign: on === true })}
                />
                {t("inspector.replay.resign")}
              </label>
            </div>
            <p className="-mt-2 text-callout text-secondary">
              {webhook?.hasSecret
                ? t("inspector.replay.resignHelp", { provider: providerNames[webhook.provider] })
                : webhook
                  ? t("inspector.replay.resignNeedsSecret", {
                      provider: providerNames[webhook.provider],
                    })
                  : t("inspector.replay.resignNotWebhook")}
            </p>
            {detail?.restored ? (
              <p className="text-callout text-secondary">{t("inspector.replay.restored")}</p>
            ) : null}
            {replay.error ? (
              <p role="alert" className="text-callout text-error">
                {toIpcError(replay.error).message}
              </p>
            ) : null}
          </form>
        ) : null}
      </SheetContent>
    </Sheet>
  );
}
