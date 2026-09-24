import { Plus, X } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Select } from "@/components/ui/select";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { TextArea } from "@/components/ui/text-area";
import { type MessageKey, t } from "@/lib/i18n";
import type {
  FaultAction,
  FaultRule,
  HeaderOp,
  NetworkPreset,
  StubMode,
  StubRule,
  TapPatch,
  TapView,
} from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useConfigureTap } from "../queries";
import { TapProtection } from "./tap-protection";

type Tab = "responses" | "headers" | "network" | "protection" | "more";
const TABS: readonly Tab[] = ["responses", "headers", "network", "protection", "more"];
const PRESETS: readonly NetworkPreset[] = ["off", "threeG", "fourG", "satellite"];
const IDLE = ["0", "15", "30", "60", "120", "480"] as const;

interface Draft {
  stubs: StubRule[];
  pausedOn: boolean;
  pausedTitle: string;
  pausedMessage: string;
  request: HeaderOp[];
  response: HeaderOp[];
  cors: boolean;
  hostHeader: string;
  /** `custom`: the tap has a simulated network no preset describes (left alone). */
  preset: NetworkPreset | "custom";
  faults: FaultRule[];
  keepAlive: string;
  watched: string;
  idle: string;
  capturing: boolean;
}

function presetOf(tap: TapView): Draft["preset"] {
  const n = tap.network;
  return n.latency || n.upBytesPerSec || n.downBytesPerSec ? "custom" : "off";
}

function draftOf(tap: TapView): Draft {
  return {
    stubs: tap.stubs,
    pausedOn: tap.paused !== null,
    pausedTitle: tap.paused?.title ?? "",
    pausedMessage: tap.paused?.message ?? "",
    request: tap.headerRules.request ?? [],
    response: tap.headerRules.response ?? [],
    cors: tap.headerRules.cors ?? false,
    hostHeader: tap.hostHeader ?? "",
    preset: presetOf(tap),
    faults: tap.faults,
    keepAlive: String(tap.sseKeepaliveSecs ?? 0),
    watched: tap.watchedPaths.join("\n"),
    idle: String(tap.idleStopMinutes ?? 0),
    capturing: tap.capturing,
  };
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Only what changed, so an untouched setting is never rewritten. */
export function patchOf(tap: TapView, draft: Draft): TapPatch {
  const before = draftOf(tap);
  const patch: TapPatch = {};
  if (!same(draft.stubs, before.stubs)) patch.stubs = draft.stubs;
  if (
    draft.pausedOn !== before.pausedOn ||
    (draft.pausedOn &&
      (draft.pausedTitle !== before.pausedTitle || draft.pausedMessage !== before.pausedMessage))
  ) {
    patch.paused = draft.pausedOn;
    if (draft.pausedOn && (draft.pausedTitle.trim() || draft.pausedMessage.trim())) {
      patch.pausedPage = {
        title: draft.pausedTitle.trim(),
        message: draft.pausedMessage.trim(),
        retryAfterSecs: tap.paused?.retryAfterSecs ?? 60,
      };
    }
  }
  if (
    !same(draft.request, before.request) ||
    !same(draft.response, before.response) ||
    draft.cors !== before.cors
  ) {
    patch.headerRules = { request: draft.request, response: draft.response, cors: draft.cors };
  }
  if (draft.hostHeader.trim() !== before.hostHeader) patch.hostHeader = draft.hostHeader.trim();
  if (draft.preset !== before.preset && draft.preset !== "custom") {
    patch.networkPreset = draft.preset;
  }
  if (!same(draft.faults, before.faults)) patch.faults = draft.faults;
  if (draft.keepAlive !== before.keepAlive) patch.sseKeepaliveSecs = Number(draft.keepAlive) || 0;
  if (draft.watched.trim() !== before.watched.trim()) {
    patch.watchedPaths = draft.watched
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
  }
  if (draft.idle !== before.idle) patch.idleStopMinutes = Number(draft.idle);
  if (draft.capturing !== before.capturing) patch.capturing = draft.capturing;
  return patch;
}

interface TapSettingsSheetProps {
  tap: TapView | null;
  open: boolean;
  onClose: () => void;
}

/**
 * Everything the inspector does for one share or route, on this computer: stubs and the
 * paused page, header rules and CORS, the simulated network and faults, stream
 * keep-alive, protection, watched paths and idle stop.
 */
export function TapSettingsSheet({ tap, open, onClose }: TapSettingsSheetProps) {
  const [tab, setTab] = useState<Tab>("responses");
  const [draft, setDraft] = useState<Draft | null>(null);
  const configure = useConfigureTap();

  // Start from the tap's settings each time the sheet opens.
  // biome-ignore lint/correctness/useExhaustiveDependencies: once per opening
  useEffect(() => {
    if (!open || !tap) return;
    setDraft(draftOf(tap));
    setTab("responses");
    configure.reset();
  }, [open, tap?.id]);

  if (!tap || !draft) return null;
  const patch = patchOf(tap, draft);
  const changed = Object.keys(patch).length > 0;
  const set = (next: Partial<Draft>) => setDraft({ ...draft, ...next });

  const save = () =>
    configure.mutate(
      { tap: tap.id, patch },
      {
        onSuccess: () => {
          toast.success(t("inspector.tap.saved", { name: tap.name }));
          onClose();
        },
      },
    );

  return (
    <Sheet open={open} onOpenChange={(next) => !next && !configure.isPending && onClose()}>
      <SheetContent
        title={t("inspector.tap.title", { name: tap.name })}
        description={t("inspector.tap.description")}
        width="lg"
        onPointerDownOutside={(event) => event.preventDefault()}
        footer={
          tab === "protection" ? (
            <SheetClose asChild>
              <Button variant="primary">{t("common.done")}</Button>
            </SheetClose>
          ) : (
            <>
              <SheetClose asChild>
                <Button disabled={configure.isPending}>{t("common.cancel")}</Button>
              </SheetClose>
              <Button
                variant="primary"
                disabled={!changed}
                pending={configure.isPending}
                onClick={save}
              >
                {t("inspector.tap.save")}
              </Button>
            </>
          )
        }
      >
        <div className="flex flex-col gap-4">
          <SegmentedControl
            label={t("inspector.tap.sections")}
            segments={TABS.map((value) => ({
              value,
              label: t(`inspector.tap.tab.${value}` as MessageKey),
            }))}
            value={tab}
            onValueChange={setTab}
            className="self-center"
          />
          {tab === "responses" ? (
            <Responses draft={draft} set={set} />
          ) : tab === "headers" ? (
            <Headers draft={draft} set={set} />
          ) : tab === "network" ? (
            <Network draft={draft} set={set} />
          ) : tab === "protection" ? (
            <TapProtection key={tap.id} tap={tap} />
          ) : (
            <More draft={draft} set={set} />
          )}
          {configure.error ? (
            <p role="alert" className="text-callout text-error">
              {toIpcError(configure.error).message}
            </p>
          ) : null}
        </div>
      </SheetContent>
    </Sheet>
  );
}

interface SectionProps {
  draft: Draft;
  set: (next: Partial<Draft>) => void;
}

function Group({
  title,
  help,
  action,
  children,
}: {
  title: string;
  help?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <h3 className="text-headline">{title}</h3>
        {action ? <div className="ml-auto">{action}</div> : null}
      </div>
      {help ? <p className="-mt-1 text-callout text-secondary">{help}</p> : null}
      {children}
    </section>
  );
}

function ToggleRow({
  id,
  label,
  help,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  help?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label htmlFor={id} className="flex items-start gap-2 text-body">
      <Switch id={id} checked={checked} onCheckedChange={onChange} />
      <span className="flex flex-col">
        {label}
        {help ? <span className="text-callout text-secondary">{help}</span> : null}
      </span>
    </label>
  );
}

const replace = <T,>(list: readonly T[], index: number, item: T) =>
  list.map((existing, at) => (at === index ? item : existing));

function contentTypeOf(stub: StubRule) {
  return stub.headers.find(([name]) => name.toLowerCase() === "content-type")?.[1] ?? "";
}

function withContentType(stub: StubRule, value: string): StubRule {
  const rest = stub.headers.filter(([name]) => name.toLowerCase() !== "content-type");
  return { ...stub, headers: value ? [["Content-Type", value], ...rest] : rest };
}

function Responses({ draft, set }: SectionProps) {
  const stubs = draft.stubs;
  const update = (index: number, stub: StubRule) => set({ stubs: replace(stubs, index, stub) });
  return (
    <>
      <Group
        title={t("inspector.tap.stubs.title")}
        help={t("inspector.tap.stubs.help")}
        action={
          <Button
            size="sm"
            onClick={() =>
              set({
                stubs: [
                  ...stubs,
                  {
                    method: null,
                    path: "/",
                    mode: "whenUnreachable",
                    status: 200,
                    headers: [["Content-Type", "application/json"]],
                    body: "{}",
                  },
                ],
              })
            }
          >
            <Plus /> {t("inspector.tap.stubs.add")}
          </Button>
        }
      >
        {stubs.length === 0 ? (
          <p className="text-callout text-secondary">{t("inspector.tap.stubs.empty")}</p>
        ) : (
          <ol className="flex flex-col gap-3">
            {stubs.map((stub, index) => (
              <li
                // biome-ignore lint/suspicious/noArrayIndexKey: rules are positional
                key={index}
                className="flex flex-col gap-2 rounded-control bg-surface-inset p-2.5"
              >
                <div className="flex items-center gap-2">
                  <Input
                    aria-label={t("inspector.tap.method")}
                    placeholder={t("inspector.tap.anyMethod")}
                    className="w-20 font-mono text-mono uppercase"
                    value={stub.method ?? ""}
                    onChange={(e) => update(index, { ...stub, method: e.target.value || null })}
                  />
                  <Input
                    aria-label={t("inspector.tap.path")}
                    className="min-w-0 flex-1 font-mono text-mono"
                    value={stub.path}
                    onChange={(e) => update(index, { ...stub, path: e.target.value })}
                  />
                  <Input
                    aria-label={t("inspector.tap.stubs.status")}
                    inputMode="numeric"
                    className="w-14 tabular"
                    value={String(stub.status)}
                    onChange={(e) =>
                      update(index, {
                        ...stub,
                        status: Number(e.target.value.replace(/\D/g, "").slice(0, 3)) || 0,
                      })
                    }
                  />
                  <IconButton
                    icon={X}
                    label={t("inspector.tap.stubs.remove")}
                    onClick={() => set({ stubs: stubs.filter((_, at) => at !== index) })}
                  />
                </div>
                <div className="flex items-center gap-2">
                  <Select
                    label={t("inspector.tap.stubs.when")}
                    options={(["always", "whenUnreachable"] as const).map((value: StubMode) => ({
                      value,
                      label: t(`inspector.tap.stubs.mode.${value}`),
                    }))}
                    value={stub.mode}
                    onValueChange={(mode) => update(index, { ...stub, mode })}
                  />
                  <Input
                    aria-label={t("inspector.tap.stubs.contentType")}
                    placeholder={t("inspector.tap.stubs.contentType")}
                    className="min-w-0 flex-1 font-mono text-mono"
                    value={contentTypeOf(stub)}
                    onChange={(e) => update(index, withContentType(stub, e.target.value))}
                  />
                </div>
                <TextArea
                  aria-label={t("inspector.tap.stubs.body")}
                  rows={3}
                  className="font-mono text-mono"
                  value={stub.body}
                  onChange={(e) => update(index, { ...stub, body: e.target.value })}
                />
              </li>
            ))}
          </ol>
        )}
      </Group>
      <Group title={t("inspector.tap.paused.title")}>
        <ToggleRow
          id="tap-paused"
          label={t("inspector.tap.paused.toggle")}
          help={t("inspector.tap.paused.help")}
          checked={draft.pausedOn}
          onChange={(pausedOn) => set({ pausedOn })}
        />
        {draft.pausedOn ? (
          <div className="flex flex-col gap-2 pl-10">
            <Input
              aria-label={t("inspector.tap.paused.heading")}
              placeholder={t("inspector.tap.paused.heading")}
              value={draft.pausedTitle}
              onChange={(e) => set({ pausedTitle: e.target.value })}
            />
            <TextArea
              aria-label={t("inspector.tap.paused.message")}
              placeholder={t("inspector.tap.paused.message")}
              rows={2}
              value={draft.pausedMessage}
              onChange={(e) => set({ pausedMessage: e.target.value })}
            />
          </div>
        ) : null}
      </Group>
    </>
  );
}

function HeaderList({
  kind,
  ops,
  onChange,
}: {
  kind: "request" | "response";
  ops: HeaderOp[];
  onChange: (ops: HeaderOp[]) => void;
}) {
  return (
    <Group
      title={t(`inspector.tap.headers.${kind}`)}
      action={
        <Button size="sm" onClick={() => onChange([...ops, { op: "set", name: "", value: "" }])}>
          <Plus /> {t("inspector.tap.headers.add")}
        </Button>
      }
    >
      {ops.length === 0 ? (
        <p className="text-callout text-secondary">{t("inspector.tap.headers.empty")}</p>
      ) : (
        <ol className="flex flex-col gap-1.5">
          {ops.map((op, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rules are positional
            <li key={index} className="flex items-center gap-2">
              <Select
                label={t("inspector.tap.headers.op")}
                options={(["set", "append", "remove"] as const).map((value) => ({
                  value,
                  label: t(`inspector.tap.headers.ops.${value}`),
                }))}
                value={op.op}
                onValueChange={(next) =>
                  onChange(
                    replace(
                      ops,
                      index,
                      next === "remove"
                        ? { op: "remove", name: op.name }
                        : { op: next, name: op.name, value: "value" in op ? op.value : "" },
                    ),
                  )
                }
                className="w-24"
              />
              <Input
                aria-label={t("inspector.tap.headers.name")}
                placeholder={t("inspector.tap.headers.name")}
                className="w-40 font-mono text-mono"
                value={op.name}
                onChange={(e) => onChange(replace(ops, index, { ...op, name: e.target.value }))}
              />
              {op.op === "remove" ? (
                <span className="min-w-0 flex-1" />
              ) : (
                <Input
                  aria-label={t("inspector.tap.headers.value")}
                  placeholder={t("inspector.tap.headers.value")}
                  className="min-w-0 flex-1 font-mono text-mono"
                  value={op.value}
                  onChange={(e) => onChange(replace(ops, index, { ...op, value: e.target.value }))}
                />
              )}
              <IconButton
                icon={X}
                label={t("inspector.tap.headers.remove")}
                onClick={() => onChange(ops.filter((_, at) => at !== index))}
              />
            </li>
          ))}
        </ol>
      )}
    </Group>
  );
}

function Headers({ draft, set }: SectionProps) {
  return (
    <>
      <Group title={t("inspector.tap.host.title")} help={t("inspector.tap.host.help")}>
        <Input
          aria-label={t("inspector.tap.host.title")}
          placeholder={t("inspector.tap.host.placeholder")}
          className="font-mono text-mono"
          value={draft.hostHeader}
          onChange={(e) => set({ hostHeader: e.target.value })}
        />
      </Group>
      <HeaderList kind="request" ops={draft.request} onChange={(request) => set({ request })} />
      <HeaderList kind="response" ops={draft.response} onChange={(response) => set({ response })} />
      <ToggleRow
        id="tap-cors"
        label={t("inspector.tap.headers.cors")}
        help={t("inspector.tap.headers.corsHelp")}
        checked={draft.cors}
        onChange={(cors) => set({ cors })}
      />
    </>
  );
}

function faultParam(action: FaultAction): string {
  switch (action.type) {
    case "status":
      return String(action.status);
    case "delay":
      return String(action.ms);
    case "timeout":
      return String(action.after_ms);
    case "reset":
      return "";
  }
}

function faultAction(type: FaultAction["type"], param: string): FaultAction {
  const n = Number(param.replace(/\D/g, "")) || 0;
  switch (type) {
    case "status":
      return { type, status: n || 503, retry_after_secs: null };
    case "delay":
      return { type, ms: n || 2000 };
    case "timeout":
      return { type, after_ms: n || 30_000 };
    case "reset":
      return { type };
  }
}

function Network({ draft, set }: SectionProps) {
  const faults = draft.faults;
  const update = (index: number, fault: FaultRule) =>
    set({ faults: replace(faults, index, fault) });
  return (
    <>
      <Group title={t("inspector.tap.network.title")} help={t("inspector.tap.network.help")}>
        <SegmentedControl
          label={t("inspector.tap.network.title")}
          segments={[
            ...PRESETS.map((value) => ({
              value: value as Draft["preset"],
              label: t(`inspector.tap.network.preset.${value}`),
            })),
            ...(draft.preset === "custom"
              ? [{ value: "custom" as const, label: t("inspector.tap.network.preset.custom") }]
              : []),
          ]}
          value={draft.preset}
          onValueChange={(preset) => set({ preset })}
          className="self-start"
        />
      </Group>
      <Group
        title={t("inspector.tap.faults.title")}
        help={t("inspector.tap.faults.help")}
        action={
          <Button
            size="sm"
            onClick={() =>
              set({
                faults: [
                  ...faults,
                  {
                    method: null,
                    path: "/*",
                    percent: 10,
                    action: { type: "status", status: 503, retry_after_secs: null },
                  },
                ],
              })
            }
          >
            <Plus /> {t("inspector.tap.faults.add")}
          </Button>
        }
      >
        {faults.length === 0 ? (
          <p className="text-callout text-secondary">{t("inspector.tap.faults.empty")}</p>
        ) : (
          <ol className="flex flex-col gap-1.5">
            {faults.map((fault, index) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: rules are positional
              <li key={index} className="flex items-center gap-2">
                <Input
                  aria-label={t("inspector.tap.method")}
                  placeholder={t("inspector.tap.anyMethod")}
                  className="w-16 font-mono text-mono uppercase"
                  value={fault.method ?? ""}
                  onChange={(e) => update(index, { ...fault, method: e.target.value || null })}
                />
                <Input
                  aria-label={t("inspector.tap.path")}
                  className="min-w-0 flex-1 font-mono text-mono"
                  value={fault.path}
                  onChange={(e) => update(index, { ...fault, path: e.target.value })}
                />
                <Input
                  aria-label={t("inspector.tap.faults.percent")}
                  inputMode="numeric"
                  className="w-12 tabular"
                  value={String(fault.percent)}
                  onChange={(e) =>
                    update(index, {
                      ...fault,
                      percent: Math.min(100, Number(e.target.value.replace(/\D/g, "")) || 0),
                    })
                  }
                />
                <span className="text-callout text-secondary">%</span>
                <Select
                  label={t("inspector.tap.faults.action")}
                  options={(["status", "reset", "delay", "timeout"] as const).map((value) => ({
                    value,
                    label: t(`inspector.tap.faults.actions.${value}`),
                  }))}
                  value={fault.action.type}
                  onValueChange={(type) =>
                    update(index, { ...fault, action: faultAction(type, "") })
                  }
                  className="w-32"
                />
                {fault.action.type === "reset" ? null : (
                  <Input
                    aria-label={
                      fault.action.type === "status"
                        ? t("inspector.tap.faults.statusCode")
                        : t("inspector.tap.faults.ms")
                    }
                    inputMode="numeric"
                    className="w-16 tabular"
                    value={faultParam(fault.action)}
                    onChange={(e) =>
                      update(index, {
                        ...fault,
                        action: faultAction(fault.action.type, e.target.value),
                      })
                    }
                  />
                )}
                <IconButton
                  icon={X}
                  label={t("inspector.tap.faults.remove")}
                  onClick={() => set({ faults: faults.filter((_, at) => at !== index) })}
                />
              </li>
            ))}
          </ol>
        )}
      </Group>
      <Group title={t("inspector.tap.keepAlive.title")} help={t("inspector.tap.keepAlive.help")}>
        <label htmlFor="tap-keepalive" className="flex items-center gap-2 text-body">
          <Input
            id="tap-keepalive"
            inputMode="numeric"
            className="w-16 tabular"
            value={draft.keepAlive}
            onChange={(e) => set({ keepAlive: e.target.value.replace(/\D/g, "").slice(0, 3) })}
          />
          {t("inspector.tap.keepAlive.seconds")}
        </label>
      </Group>
    </>
  );
}

function More({ draft, set }: SectionProps) {
  return (
    <>
      <ToggleRow
        id="tap-capturing"
        label={t("inspector.tap.capturing")}
        help={t("inspector.tap.capturingHelp")}
        checked={draft.capturing}
        onChange={(capturing) => set({ capturing })}
      />
      <Group title={t("inspector.tap.watched.title")} help={t("inspector.tap.watched.help")}>
        <TextArea
          aria-label={t("inspector.tap.watched.title")}
          rows={3}
          placeholder="/webhooks/*"
          className="font-mono text-mono"
          value={draft.watched}
          onChange={(e) => set({ watched: e.target.value })}
        />
      </Group>
      <Group title={t("inspector.tap.idle.title")} help={t("inspector.tap.idle.help")}>
        <Select
          label={t("inspector.tap.idle.title")}
          options={IDLE.map((value) => ({
            value,
            label:
              value === "0"
                ? t("inspector.settings.never")
                : t("inspector.settings.minutes", { count: Number(value) }),
          }))}
          value={IDLE.includes(draft.idle as (typeof IDLE)[number]) ? draft.idle : "0"}
          onValueChange={(idle) => set({ idle })}
          className="w-36 self-start"
        />
      </Group>
    </>
  );
}
