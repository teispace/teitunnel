import { CircleCheck, ExternalLink, LockKeyhole, TriangleAlert } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { toast } from "sonner";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Disclosure } from "@/components/ui/disclosure";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { TextArea } from "@/components/ui/text-area";
import {
  missingNeeds,
  PermissionFix,
  type PermissionNeed,
  useCapabilities,
  ZeroTrustFix,
} from "@/features/accounts";
import { ServicePicker } from "@/features/quick-share";
import { errorLink } from "@/lib/error-help";
import { type MessageKey, t, translate } from "@/lib/i18n";
import type {
  Change,
  OriginOptions,
  Outcome,
  PlanView,
  RouteView,
  TunnelView,
  ZoneRef,
} from "@/lib/ipc/bindings";
import { type IpcError, toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { formatAllowed, parseAllowed } from "../access";
import { applyDirectly, useApply, usePreview, useVerify } from "../queries";
import { HostnameInput, joinHostname } from "./hostname-input";
import { hasOriginSettings, OriginSettings, originSettingsToSend } from "./origin-settings";
import { PlanSteps } from "./plan-steps";

export type SheetMode =
  | { kind: "add" }
  | { kind: "edit"; route: RouteView }
  | { kind: "remove"; route: RouteView }
  /** Put back Teitunnel's routes on a tunnel edited elsewhere. */
  | { kind: "restore"; tunnelId?: string | null }
  /** Delete one of this Mac's tunnels (the default one when no id is given). */
  | { kind: "removeTunnel"; tunnelId?: string | null }
  | { kind: "createTunnel" }
  /** Load balance a route across the tunnels that route its hostname, or stop. */
  | { kind: "balance"; route: RouteView }
  | { kind: "unbalance"; route: RouteView }
  | { kind: "addNetwork" }
  | { kind: "removeNetwork"; network: string }
  /** A Doctor fix: any change, reviewed like the others, on the issue's tunnel. */
  | { kind: "fix"; change: Change; label: string; tunnelId?: string | null };

type Stage = "form" | "review" | "applying" | "done";

const titles: Record<SheetMode["kind"], MessageKey> = {
  add: "routeSheet.title.add",
  edit: "routeSheet.title.edit",
  remove: "routeSheet.title.remove",
  restore: "routeSheet.title.restore",
  removeTunnel: "routeSheet.title.removeTunnel",
  createTunnel: "routeSheet.title.createTunnel",
  balance: "routeSheet.title.balance",
  unbalance: "routeSheet.title.unbalance",
  addNetwork: "routeSheet.title.addNetwork",
  removeNetwork: "routeSheet.title.removeNetwork",
  fix: "routeSheet.title.fix",
};

const applyLabels: Record<SheetMode["kind"], MessageKey> = {
  add: "routeSheet.apply.add",
  edit: "routeSheet.apply.edit",
  remove: "routeSheet.apply.remove",
  restore: "routeSheet.apply.restore",
  removeTunnel: "routeSheet.apply.removeTunnel",
  createTunnel: "routeSheet.apply.createTunnel",
  balance: "routeSheet.apply.balance",
  unbalance: "routeSheet.apply.unbalance",
  addNetwork: "routeSheet.apply.addNetwork",
  removeNetwork: "routeSheet.apply.removeNetwork",
  fix: "routeSheet.apply.fix",
};

/** Errors the sheet can fix in place instead of reporting. */
const ACCESS_PERMISSION = "core.error.observe.accessPermission";
const PERMISSION = "core.error.cloudflare.permission";
const ZERO_TRUST = "core.error.plan.zeroTrustNotSetUp";

/** The Cloudflare page that resolves `error`, as a button (see `lib/error-help`). */
function ErrorLink({ error, accountId }: { error: IpcError | null; accountId: string }) {
  const link = errorLink(error?.key);
  if (!link) return null;
  return (
    <div>
      <Button size="sm" onClick={() => void openUrl(link.url.replace(":account", accountId))}>
        {t(link.label)} <ExternalLink />
      </Button>
    </div>
  );
}

function zoneOf(hostname: string, zones: readonly ZoneRef[]): string | undefined {
  const host = hostname.toLowerCase();
  return zones.find((z) => host === z.name || host.endsWith(`.${z.name}`))?.name;
}

function shortOrigin(origin: string) {
  return origin.replace(/^http:\/\/localhost:/, "");
}

interface Form {
  hostname: string;
  origin: string;
  path: string;
  /** Who can sign in, as typed; `null`: no login. */
  allowed: string | null;
  /** A private network, as typed. */
  network: string;
  /** A new tunnel's name, as typed. */
  tunnelName: string;
  /** Origin settings (`originRequest`). */
  options: OriginOptions;
}

const emptyForm: Form = {
  hostname: "",
  origin: "",
  path: "",
  allowed: null,
  network: "",
  tunnelName: "",
  options: {},
};

/** Modes that start with a form (the others go straight to review). */
const hasForm = (kind: SheetMode["kind"]) =>
  kind === "add" || kind === "edit" || kind === "addNetwork" || kind === "createTunnel";

/** The tunnel a mode changes: the route's, the one named, or `null` (the default). */
function tunnelOf(mode: SheetMode): string | null {
  switch (mode.kind) {
    case "edit":
    case "remove":
    case "balance":
    case "unbalance":
      return mode.route.tunnelId;
    case "restore":
    case "removeTunnel":
    case "fix":
      return mode.tunnelId ?? null;
    default:
      return null;
  }
}

/** The change a sheet applies, from what's in its form. */
function changeFor(mode: SheetMode, form: Form) {
  const route = {
    hostname: form.hostname,
    origin: form.origin,
    path: form.path.trim() || null,
    access: form.allowed === null ? null : parseAllowed(form.allowed),
  };
  switch (mode.kind) {
    case "add":
      return {
        type: "addRoute",
        route: { ...route, options: originSettingsToSend(form.options) },
      } satisfies Change;
    case "edit":
      // The form shows every setting, so an edit sends them all (clearing one works).
      return {
        type: "updateRoute",
        hostname: mode.route.hostname,
        path: mode.route.path,
        route: { ...route, options: form.options },
      } satisfies Change;
    case "remove":
      return {
        type: "removeRoute",
        hostname: mode.route.hostname,
        path: mode.route.path,
      } satisfies Change;
    case "restore":
      return { type: "restoreConfig" } satisfies Change;
    case "removeTunnel":
      return { type: "removeTunnel" } satisfies Change;
    case "createTunnel":
      return { type: "createTunnel", name: form.tunnelName } satisfies Change;
    case "balance":
      return { type: "balanceRoute", hostname: mode.route.hostname } satisfies Change;
    case "unbalance":
      return { type: "unbalanceRoute", hostname: mode.route.hostname } satisfies Change;
    case "addNetwork":
      return { type: "addNetwork", network: form.network } satisfies Change;
    case "removeNetwork":
      return { type: "removeNetwork", network: mode.network } satisfies Change;
    case "fix":
      return mode.change;
  }
}

/** The change that puts things back after `change` was applied (for Undo). */
function inverseOf(mode: SheetMode, change: Change): Change | null {
  switch (change.type) {
    case "addRoute":
      return { type: "removeRoute", hostname: change.route.hostname, path: change.route.path };
    case "updateRoute":
      return mode.kind === "edit"
        ? {
            type: "updateRoute",
            hostname: change.route.hostname,
            path: change.route.path,
            route: {
              hostname: mode.route.hostname,
              path: mode.route.path,
              origin: mode.route.origin,
              access: mode.route.access,
              options: mode.route.options,
            },
          }
        : null;
    case "removeRoute":
      return mode.kind === "remove"
        ? {
            type: "addRoute",
            route: {
              hostname: mode.route.hostname,
              path: mode.route.path,
              origin: mode.route.origin,
              access: mode.route.access,
              options: originSettingsToSend(mode.route.options),
            },
          }
        : null;
    case "addNetwork":
      return { type: "removeNetwork", network: change.network };
    case "balanceRoute":
      return { type: "unbalanceRoute", hostname: change.hostname };
    case "unbalanceRoute":
      return { type: "balanceRoute", hostname: change.hostname };
    case "removeNetwork":
      return mode.kind === "removeNetwork" ? { type: "addNetwork", network: change.network } : null;
    default:
      return null;
  }
}

const doneMessages: Record<SheetMode["kind"], MessageKey> = {
  add: "routeSheet.done.add",
  edit: "routeSheet.done.edit",
  remove: "routeSheet.done.remove",
  restore: "routeSheet.done.restore",
  removeTunnel: "routeSheet.done.removeTunnel",
  createTunnel: "routeSheet.done.createTunnel",
  balance: "routeSheet.done.balance",
  unbalance: "routeSheet.done.unbalance",
  addNetwork: "routeSheet.done.addNetwork",
  removeNetwork: "routeSheet.done.removeNetwork",
  fix: "routeSheet.done.fix",
};

interface RouteSheetProps {
  accountId: string;
  zones: readonly ZoneRef[];
  /** This Mac's tunnels; with more than one, a new route can go on any of them. */
  tunnels?: readonly TunnelView[];
  /** What the sheet does; `null` closes it. */
  mode: SheetMode | null;
  onClose: () => void;
}

/**
 * Every routes change goes through this sheet: fill in (add/edit) → review the plan →
 * apply with live progress → check the URL works. Nothing changes before Apply.
 */
export function RouteSheet({ accountId, zones, tunnels = [], mode, onClose }: RouteSheetProps) {
  const open = mode !== null;
  const [stage, setStage] = useState<Stage>("form");
  const [hostname, setHostname] = useState("");
  const [origin, setOrigin] = useState("");
  const [path, setPath] = useState("");
  const [allowed, setAllowed] = useState<string | null>(null);
  const [network, setNetwork] = useState("");
  const [tunnelName, setTunnelName] = useState("");
  const [options, setOptions] = useState<OriginOptions>({});
  /** The tunnel the change is on (`null`: the default one). */
  const [tunnelId, setTunnelId] = useState<string | null>(null);
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [change, setChange] = useState<Change | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const preview = usePreview(accountId);
  const apply = useApply(accountId);
  const verify = useVerify(accountId);
  const caps = useCapabilities(accountId).data;

  const review = (next: Change, why: string | null = null, tunnel = tunnelId) => {
    setChange(next);
    setNotice(why);
    setConfirmed(false);
    preview.mutate(
      { change: next, tunnelId: tunnel },
      {
        onSuccess: (result) => {
          setPlan(result);
          setStage("review");
        },
      },
    );
  };

  // Reset whenever the sheet opens; changes without a form go straight to review.
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs once per opening
  useEffect(() => {
    if (!mode) return;
    const route = mode.kind === "edit" || mode.kind === "remove" ? mode.route : null;
    setHostname(route?.hostname ?? joinHostname("", zones[0]?.name ?? ""));
    setOrigin(route ? shortOrigin(route.origin) : "");
    setPath(route?.path ?? "");
    setAllowed(route?.access ? formatAllowed(route.access) : null);
    setOptions(route?.options ?? {});
    setNetwork("");
    setTunnelName("");
    const tunnel = tunnelOf(mode);
    setTunnelId(tunnel);
    setPlan(null);
    setOutcome(null);
    setNotice(null);
    preview.reset();
    apply.reset();
    verify.reset();
    if (hasForm(mode.kind)) {
      setStage("form");
    } else {
      setStage("review");
      review(changeFor(mode, emptyForm), null, tunnel);
    }
  }, [mode]);

  const submitForm = (event: FormEvent) => {
    event.preventDefault();
    if (mode)
      review(changeFor(mode, { hostname, origin, path, allowed, network, tunnelName, options }));
  };

  const runApply = () => {
    if (!mode || !plan || !change) return;
    setStage("applying");
    apply.mutate(
      { change, tunnelId, fingerprint: plan.fingerprint, confirmed },
      {
        onSuccess: (result) => {
          setOutcome(result);
          if (result.type !== "applied") return;
          const target = result.verify[0];
          const undo = inverseOf(mode, change);
          toast.success(t(doneMessages[mode.kind]), {
            duration: 10_000,
            ...(undo
              ? {
                  action: {
                    label: t("common.undo"),
                    onClick: () =>
                      void applyDirectly(accountId, undo, tunnelId).catch((error: unknown) =>
                        toast.error(t("routeSheet.undoFailed"), {
                          description: toIpcError(error).message,
                        }),
                      ),
                  },
                }
              : {}),
          });
          if (target) {
            setStage("done");
            verify.mutate({ hostname: target, wait: true });
          } else {
            onClose();
          }
        },
        onError: (error) => {
          const failure = toIpcError(error);
          if (failure.code === "conflict") {
            review(change, failure.message);
          } else {
            setStage("review");
          }
        },
      },
    );
  };

  const fieldError = (field: string): string | null => {
    const error = preview.error ? toIpcError(preview.error) : null;
    return error?.field === field ? error.message : null;
  };
  const kind = mode?.kind ?? "add";

  // What this change needs from the credential. Checked before review (a known gap
  // disables Review) and listed when Cloudflare refuses, with the fix in place.
  const routeZone =
    mode?.kind === "edit" || mode?.kind === "remove" ? mode.route.zone : zoneOf(hostname, zones);
  const needs: PermissionNeed[] =
    kind === "balance" || kind === "unbalance"
      ? [{ kind: "loadBalancing" }]
      : [
          { kind: "tunnels" },
          ...(routeZone &&
          kind !== "addNetwork" &&
          kind !== "removeNetwork" &&
          kind !== "createTunnel"
            ? [{ kind: "dns" as const, zone: routeZone }]
            : []),
          ...(allowed !== null && hasForm(kind) ? [{ kind: "access" as const }] : []),
        ];
  const gaps = stage === "form" && caps ? missingNeeds(caps, needs) : [];
  const failure = preview.error
    ? toIpcError(preview.error)
    : apply.error
      ? toIpcError(apply.error)
      : null;
  const refusedNeeds: PermissionNeed[] | null =
    failure?.key === ACCESS_PERMISSION
      ? [{ kind: "access" }]
      : failure?.key === PERMISSION
        ? needs
        : null;
  // After the fix, run the same review again.
  const retry = () => {
    if (!mode || !failure) return;
    preview.reset();
    apply.reset();
    review(
      stage === "review" && change
        ? change
        : changeFor(mode, { hostname, origin, path, allowed, network, tunnelName, options }),
    );
  };
  const fixCard = refusedNeeds ? (
    <PermissionFix accountId={accountId} needs={refusedNeeds} refused onReady={retry} />
  ) : failure?.key === ZERO_TRUST ? (
    <ZeroTrustFix onRetry={retry} retrying={preview.isPending} />
  ) : gaps.length > 0 ? (
    <PermissionFix accountId={accountId} needs={needs} />
  ) : null;
  const generalError: IpcError | null = fixCard
    ? null
    : preview.error &&
        !["hostname", "origin", "path", "access", "network", "tunnelName"].includes(
          toIpcError(preview.error).field ?? "",
        )
      ? toIpcError(preview.error)
      : apply.error && toIpcError(apply.error).code !== "conflict"
        ? toIpcError(apply.error)
        : null;

  const destructive =
    kind === "remove" ||
    kind === "removeTunnel" ||
    kind === "removeNetwork" ||
    (mode?.kind === "fix" &&
      (mode.change.type === "deleteRecord" ||
        mode.change.type === "removeTunnel" ||
        mode.change.type === "removeLogin"));
  const failed = outcome && outcome.type !== "applied" ? outcome : null;
  const url = stage === "done" && verify.variables ? `https://${verify.variables.hostname}` : null;

  const footer = (() => {
    switch (stage) {
      case "form":
        return (
          <>
            <SheetClose asChild>
              <Button>{t("common.cancel")}</Button>
            </SheetClose>
            <Button
              variant="primary"
              type="submit"
              form="route-form"
              pending={preview.isPending}
              disabled={
                (kind === "addNetwork"
                  ? network
                  : kind === "createTunnel"
                    ? tunnelName
                    : origin
                ).trim() === "" || gaps.length > 0
              }
            >
              {preview.isPending ? t("routeSheet.checking") : t("routeSheet.review")}
            </Button>
          </>
        );
      case "review":
        return (
          <>
            {hasForm(kind) ? (
              <Button className="mr-auto" onClick={() => setStage("form")}>
                {t("routeSheet.back")}
              </Button>
            ) : null}
            <SheetClose asChild>
              <Button>{t("common.cancel")}</Button>
            </SheetClose>
            <Button
              variant={destructive ? "destructive" : "primary"}
              disabled={
                !plan ||
                plan.steps.length === 0 ||
                (plan.requiresConfirmation && !confirmed) ||
                preview.isPending
              }
              onClick={runApply}
            >
              {t(applyLabels[kind])}
            </Button>
          </>
        );
      case "applying":
        return failed ? (
          <>
            <Button className="mr-auto" onClick={() => change && review(change)}>
              {t("routeSheet.reviewAgain")}
            </Button>
            <SheetClose asChild>
              <Button variant="primary">{t("common.close")}</Button>
            </SheetClose>
          </>
        ) : (
          <Button pending>{t("routeSheet.applying")}</Button>
        );
      case "done":
        return (
          <>
            {verify.data && !verify.data.failure ? null : (
              <Button
                className="mr-auto"
                pending={verify.isPending}
                onClick={() =>
                  verify.variables &&
                  verify.mutate({ hostname: verify.variables.hostname, wait: false })
                }
              >
                {t("routeSheet.testAgain")}
              </Button>
            )}
            <SheetClose asChild>
              <Button variant="primary">{t("common.done")}</Button>
            </SheetClose>
          </>
        );
    }
  })();

  return (
    <Sheet open={open} onOpenChange={(next) => !next && stage !== "applying" && onClose()}>
      <SheetContent
        title={mode?.kind === "fix" ? mode.label : t(titles[kind])}
        description={
          stage === "form"
            ? kind === "addNetwork"
              ? t("routeSheet.description.network")
              : kind === "createTunnel"
                ? t("routeSheet.description.createTunnel")
                : t("routeSheet.description.route")
            : stage === "review"
              ? t("routeSheet.description.review")
              : undefined
        }
        footer={footer}
        onEscapeKeyDown={(event) => stage === "applying" && event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        {stage === "form" && kind === "createTunnel" ? (
          <form id="route-form" onSubmit={submitForm} className="flex flex-col gap-4">
            <Field
              label={t("routeSheet.tunnelName.label")}
              error={fieldError("tunnelName")}
              help={t("routeSheet.tunnelName.help")}
            >
              {(control) => (
                <Input
                  {...control}
                  autoFocus
                  placeholder={t("routeSheet.tunnelName.placeholder")}
                  autoComplete="off"
                  spellCheck={false}
                  maxLength={64}
                  value={tunnelName}
                  onChange={(event) => setTunnelName(event.target.value)}
                />
              )}
            </Field>
            {fixCard}
            {generalError ? (
              <p role="alert" className="text-callout text-error">
                {generalError.message}
              </p>
            ) : null}
          </form>
        ) : stage === "form" && kind === "addNetwork" ? (
          <form id="route-form" onSubmit={submitForm} className="flex flex-col gap-4">
            <Field
              label={t("routeSheet.network.label")}
              error={fieldError("network")}
              help={t("routeSheet.network.help")}
            >
              {(control) => (
                <Input
                  {...control}
                  autoFocus
                  placeholder="192.168.1.0/24"
                  autoComplete="off"
                  spellCheck={false}
                  className="font-mono text-mono"
                  value={network}
                  onChange={(event) => setNetwork(event.target.value)}
                />
              )}
            </Field>
            {fixCard}
            {generalError ? (
              <p role="alert" className="text-callout text-error">
                {generalError.message}
              </p>
            ) : null}
          </form>
        ) : stage === "form" ? (
          <form id="route-form" onSubmit={submitForm} className="flex flex-col gap-4">
            <Field
              label={t("routeSheet.service.label")}
              error={fieldError("origin")}
              help={t("routeSheet.service.help")}
            >
              {(control) => (
                <ServicePicker
                  autoFocus={kind === "add"}
                  value={origin}
                  onChange={setOrigin}
                  invalid={control["aria-invalid"] === true}
                  describedBy={control["aria-describedby"]}
                />
              )}
            </Field>
            <Field label={t("routeSheet.hostname.label")} error={fieldError("hostname")}>
              {(control) => (
                <HostnameInput
                  id={control.id}
                  zones={zones}
                  value={hostname}
                  onChange={setHostname}
                  invalid={control["aria-invalid"] === true}
                  describedBy={control["aria-describedby"]}
                  autoFocus={kind === "edit"}
                />
              )}
            </Field>
            {fieldError("hostname") ? <ErrorLink error={failure} accountId={accountId} /> : null}
            {kind === "add" && tunnels.length > 1 ? (
              <Field label={t("routeSheet.tunnel.label")} help={t("routeSheet.tunnel.help")}>
                {(control) => (
                  <Select
                    id={control.id}
                    label={t("routeSheet.tunnel.label")}
                    options={tunnels.map((tunnel) => ({ value: tunnel.id, label: tunnel.name }))}
                    value={tunnelId ?? tunnels.find((tunnel) => tunnel.isDefault)?.id ?? ""}
                    onValueChange={(id) =>
                      setTunnelId(tunnels.find((tunnel) => tunnel.id === id)?.isDefault ? null : id)
                    }
                  />
                )}
              </Field>
            ) : null}
            <Disclosure
              title={t("routeSheet.advanced")}
              defaultOpen={path !== "" || allowed !== null || hasOriginSettings(options)}
            >
              <div className="flex flex-col gap-4">
                <Field
                  label={t("routeSheet.path.label")}
                  error={fieldError("path")}
                  help={t("routeSheet.path.help")}
                >
                  {(control) => (
                    <Input
                      {...control}
                      placeholder="^/api/"
                      autoComplete="off"
                      spellCheck={false}
                      className="font-mono text-mono"
                      value={path}
                      onChange={(event) => setPath(event.target.value)}
                    />
                  )}
                </Field>
                <label htmlFor="route-login" className="flex items-center gap-2 text-body">
                  <Checkbox
                    id="route-login"
                    checked={allowed !== null}
                    onCheckedChange={(value) => setAllowed(value === true ? (allowed ?? "") : null)}
                  />
                  {t("routeSheet.login.toggle")}
                </label>
                {allowed !== null ? (
                  <Field
                    label={t("routeSheet.login.label")}
                    error={fieldError("access")}
                    help={t("routeSheet.login.help")}
                  >
                    {(control) => (
                      <TextArea
                        {...control}
                        rows={2}
                        placeholder="me@example.com, @example.com"
                        autoComplete="off"
                        value={allowed}
                        onChange={(event) => setAllowed(event.target.value)}
                      />
                    )}
                  </Field>
                ) : null}
                <Disclosure
                  title={t("routeSheet.origin.title")}
                  defaultOpen={hasOriginSettings(options) || fieldError("options") !== null}
                >
                  <OriginSettings
                    value={options}
                    onChange={setOptions}
                    error={fieldError("options") ?? undefined}
                  />
                </Disclosure>
              </div>
            </Disclosure>
            {fixCard}
            {generalError ? (
              <p role="alert" className="text-callout text-error">
                {generalError.message}
              </p>
            ) : null}
          </form>
        ) : null}

        {stage === "review" ? (
          <div className="flex flex-col gap-3" aria-busy={preview.isPending}>
            {notice ? (
              <p role="status" className="text-callout text-secondary">
                {notice}
              </p>
            ) : null}
            {plan && !preview.isPending ? (
              plan.steps.length === 0 ? (
                <p className="text-body text-secondary">{t("routeSheet.nothingToChange")}</p>
              ) : (
                <PlanSteps steps={plan.steps} warnings={plan.warnings} />
              )
            ) : generalError || preview.isError ? null : (
              <div className="flex items-center gap-2 text-body text-secondary">
                <Spinner className="size-3.5" /> {t("routeSheet.reading")}
              </div>
            )}
            {fixCard}
            {plan?.requiresConfirmation && !preview.isPending ? (
              <label htmlFor="route-confirm" className="flex items-center gap-2 text-body">
                <Checkbox
                  id="route-confirm"
                  checked={confirmed}
                  onCheckedChange={(value) => setConfirmed(value === true)}
                />
                {plan.warnings.some((w) => w.type === "publicNetwork")
                  ? t("routeSheet.confirm.publicNetwork")
                  : t("routeSheet.confirm.records")}
              </label>
            ) : null}
            {generalError ? (
              <p role="alert" className="text-callout text-error">
                {generalError.message}
                {generalError.hint ? (
                  <span className="block text-secondary">{generalError.hint}</span>
                ) : null}
              </p>
            ) : null}
            <ErrorLink error={generalError} accountId={accountId} />
          </div>
        ) : null}

        {stage === "applying" && plan ? (
          <div className="flex flex-col gap-3" aria-live="polite">
            <PlanSteps steps={plan.steps} states={apply.steps} />
            {failed ? (
              <div role="alert" className="flex flex-col gap-1 text-callout">
                <p className="text-error">{translate(failed.error)}</p>
                {failed.type === "rolledBack" ? (
                  <p className="text-secondary">{t("routeSheet.rolledBack")}</p>
                ) : (
                  <>
                    <p className="text-secondary">{t("routeSheet.leftovers")}</p>
                    <ul className="list-disc pl-5 text-secondary">
                      {failed.leftovers.map((leftover) => {
                        const text = translate(leftover);
                        return <li key={text}>{text}</li>;
                      })}
                    </ul>
                  </>
                )}
              </div>
            ) : null}
          </div>
        ) : null}

        {stage === "done" && url ? (
          <div className="flex flex-col items-center gap-3 py-4 text-center" aria-live="polite">
            {verify.isPending || !verify.data ? (
              <>
                <Spinner className="size-6" label={t("routeSheet.verify.checking")} />
                <p className="text-body text-secondary">
                  {t("routeSheet.verify.pending", { url })}
                </p>
              </>
            ) : verify.data.failure ? (
              <>
                <TriangleAlert aria-hidden className="size-7 text-warning" strokeWidth={1.5} />
                <p className="max-w-sm text-body">
                  {verify.data.message ? translate(verify.data.message) : null}
                </p>
                <CopyField label={t("common.url")} value={url} className="w-full max-w-sm" />
              </>
            ) : verify.data.protected ? (
              <>
                <LockKeyhole aria-hidden className="size-7 text-healthy" strokeWidth={1.5} />
                <p className="text-headline">{t("routeSheet.verify.protected")}</p>
                <p className="max-w-sm text-body text-secondary">
                  {t("routeSheet.verify.protectedDetail")}
                </p>
                <div className="flex w-full max-w-sm items-center gap-2">
                  <CopyField label={t("common.url")} value={url} className="min-w-0 flex-1" />
                  <Button onClick={() => void openUrl(url)}>
                    {t("common.open")} <ExternalLink />
                  </Button>
                </div>
              </>
            ) : (
              <>
                <CircleCheck aria-hidden className="size-7 text-healthy" strokeWidth={1.5} />
                <p className="text-headline">{t("routeSheet.verify.works")}</p>
                <div className="flex w-full max-w-sm items-center gap-2">
                  <CopyField label={t("common.url")} value={url} className="min-w-0 flex-1" />
                  <Button onClick={() => void openUrl(url)}>
                    {t("common.open")} <ExternalLink />
                  </Button>
                </div>
              </>
            )}
          </div>
        ) : null}
      </SheetContent>
    </Sheet>
  );
}
