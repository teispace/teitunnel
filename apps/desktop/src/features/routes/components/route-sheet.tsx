import { CircleCheck, ExternalLink, LockKeyhole, TriangleAlert } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { toast } from "sonner";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Disclosure } from "@/components/ui/disclosure";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { TextArea } from "@/components/ui/text-area";
import { ServicePicker } from "@/features/quick-share";
import type { Change, Outcome, PlanView, RouteView, ZoneRef } from "@/lib/ipc/bindings";
import { type IpcError, toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { formatAllowed, parseAllowed } from "../access";
import { applyDirectly, useApply, usePreview, useVerify } from "../queries";
import { HostnameInput, joinHostname } from "./hostname-input";
import { PlanSteps } from "./plan-steps";

export type SheetMode =
  | { kind: "add" }
  | { kind: "edit"; route: RouteView }
  | { kind: "remove"; route: RouteView }
  | { kind: "restore" }
  | { kind: "removeTunnel" }
  | { kind: "addNetwork" }
  | { kind: "removeNetwork"; network: string }
  /** A Doctor fix: any change, reviewed like the others. */
  | { kind: "fix"; change: Change; label: string };

type Stage = "form" | "review" | "applying" | "done";

const titles: Record<SheetMode["kind"], string> = {
  add: "New Route",
  edit: "Edit Route",
  remove: "Remove Route",
  restore: "Restore Routes",
  removeTunnel: "Delete Tunnel",
  addNetwork: "Share a Private Network",
  removeNetwork: "Stop Sharing Network",
  fix: "Fix Issue",
};

const applyLabels: Record<SheetMode["kind"], string> = {
  add: "Add Route",
  edit: "Save",
  remove: "Remove",
  restore: "Restore",
  removeTunnel: "Delete Tunnel",
  addNetwork: "Share",
  removeNetwork: "Stop Sharing",
  fix: "Apply",
};

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
}

const emptyForm: Form = { hostname: "", origin: "", path: "", allowed: null, network: "" };

/** Modes that start with a form (the others go straight to review). */
const hasForm = (kind: SheetMode["kind"]) =>
  kind === "add" || kind === "edit" || kind === "addNetwork";

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
      return { type: "addRoute", route } satisfies Change;
    case "edit":
      return {
        type: "updateRoute",
        hostname: mode.route.hostname,
        path: mode.route.path,
        route,
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
            },
          }
        : null;
    case "addNetwork":
      return { type: "removeNetwork", network: change.network };
    case "removeNetwork":
      return mode.kind === "removeNetwork" ? { type: "addNetwork", network: change.network } : null;
    default:
      return null;
  }
}

const doneMessages: Partial<Record<SheetMode["kind"], string>> = {
  add: "Route added",
  edit: "Route updated",
  remove: "Route removed",
  restore: "Routes restored",
  removeTunnel: "Tunnel deleted",
  addNetwork: "Network shared",
  removeNetwork: "Network no longer shared",
  fix: "Fixed",
};

interface RouteSheetProps {
  accountId: string;
  zones: readonly ZoneRef[];
  /** What the sheet does; `null` closes it. */
  mode: SheetMode | null;
  onClose: () => void;
}

/**
 * Every routes change goes through this sheet: fill in (add/edit) → review the plan →
 * apply with live progress → check the URL works. Nothing changes before Apply.
 */
export function RouteSheet({ accountId, zones, mode, onClose }: RouteSheetProps) {
  const open = mode !== null;
  const [stage, setStage] = useState<Stage>("form");
  const [hostname, setHostname] = useState("");
  const [origin, setOrigin] = useState("");
  const [path, setPath] = useState("");
  const [allowed, setAllowed] = useState<string | null>(null);
  const [network, setNetwork] = useState("");
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [change, setChange] = useState<Change | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const preview = usePreview(accountId);
  const apply = useApply(accountId);
  const verify = useVerify(accountId);

  const review = (next: Change, why: string | null = null) => {
    setChange(next);
    setNotice(why);
    setConfirmed(false);
    preview.mutate(next, {
      onSuccess: (result) => {
        setPlan(result);
        setStage("review");
      },
    });
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
    setNetwork("");
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
      review(changeFor(mode, emptyForm));
    }
  }, [mode]);

  const submitForm = (event: FormEvent) => {
    event.preventDefault();
    if (mode) review(changeFor(mode, { hostname, origin, path, allowed, network }));
  };

  const runApply = () => {
    if (!mode || !plan || !change) return;
    setStage("applying");
    apply.mutate(
      { change, fingerprint: plan.fingerprint, confirmed },
      {
        onSuccess: (result) => {
          setOutcome(result);
          if (result.type !== "applied") return;
          const target = result.verify[0];
          const undo = inverseOf(mode, change);
          toast.success(doneMessages[mode.kind] ?? "Done", {
            duration: 10_000,
            ...(undo
              ? {
                  action: {
                    label: "Undo",
                    onClick: () =>
                      void applyDirectly(accountId, undo).catch((error: unknown) =>
                        toast.error("Couldn't undo", {
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
  const generalError: IpcError | null =
    preview.error &&
    !["hostname", "origin", "path", "access", "network"].includes(
      toIpcError(preview.error).field ?? "",
    )
      ? toIpcError(preview.error)
      : apply.error && toIpcError(apply.error).code !== "conflict"
        ? toIpcError(apply.error)
        : null;

  const kind = mode?.kind ?? "add";
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
              <Button>Cancel</Button>
            </SheetClose>
            <Button
              variant="primary"
              type="submit"
              form="route-form"
              disabled={
                preview.isPending || (kind === "addNetwork" ? network : origin).trim() === ""
              }
            >
              {preview.isPending ? "Checking…" : "Review"}
            </Button>
          </>
        );
      case "review":
        return (
          <>
            {hasForm(kind) ? (
              <Button className="mr-auto" onClick={() => setStage("form")}>
                Back
              </Button>
            ) : null}
            <SheetClose asChild>
              <Button>Cancel</Button>
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
              {applyLabels[kind]}
            </Button>
          </>
        );
      case "applying":
        return failed ? (
          <>
            <Button className="mr-auto" onClick={() => change && review(change)}>
              Review Again
            </Button>
            <SheetClose asChild>
              <Button variant="primary">Close</Button>
            </SheetClose>
          </>
        ) : (
          <Button disabled>Applying…</Button>
        );
      case "done":
        return (
          <>
            {verify.data && !verify.data.failure ? null : (
              <Button
                className="mr-auto"
                disabled={verify.isPending}
                onClick={() =>
                  verify.variables &&
                  verify.mutate({ hostname: verify.variables.hostname, wait: false })
                }
              >
                Test Again
              </Button>
            )}
            <SheetClose asChild>
              <Button variant="primary">Done</Button>
            </SheetClose>
          </>
        );
    }
  })();

  return (
    <Sheet open={open} onOpenChange={(next) => !next && stage !== "applying" && onClose()}>
      <SheetContent
        title={mode?.kind === "fix" ? mode.label : titles[kind]}
        description={
          stage === "form"
            ? kind === "addNetwork"
              ? "Let devices running Cloudflare WARP reach addresses on this Mac's network."
              : "Send a hostname on your domain to a service on this Mac."
            : stage === "review"
              ? "Review what will change in Cloudflare. Nothing changes until you apply."
              : undefined
        }
        footer={footer}
        onEscapeKeyDown={(event) => stage === "applying" && event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        {stage === "form" && kind === "addNetwork" ? (
          <form id="route-form" onSubmit={submitForm} className="flex flex-col gap-4">
            <Field
              label="Network"
              error={fieldError("network")}
              help="An address or a range on this Mac's network, like 192.168.1.0/24. Anyone signed in to WARP with your Zero Trust organization can reach it through this Mac."
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
            {generalError ? (
              <p role="alert" className="text-callout text-error">
                {generalError.message}
              </p>
            ) : null}
          </form>
        ) : stage === "form" ? (
          <form id="route-form" onSubmit={submitForm} className="flex flex-col gap-4">
            <Field label="Service" error={fieldError("origin")} help="A port or an address.">
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
            <Field label="Public hostname" error={fieldError("hostname")}>
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
            <Disclosure title="Advanced" defaultOpen={path !== "" || allowed !== null}>
              <div className="flex flex-col gap-4">
                <Field
                  label="Path"
                  error={fieldError("path")}
                  help="Only requests whose path matches this pattern, e.g. ^/api/. Leave empty for all."
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
                  Require a login
                </label>
                {allowed !== null ? (
                  <Field
                    label="Who can sign in"
                    error={fieldError("access")}
                    help="Email addresses, or @domain for everyone there. Visitors get a one-time code by email. Uses Cloudflare Zero Trust (free)."
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
              </div>
            </Disclosure>
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
                <p className="text-body text-secondary">Nothing to change: it's already set up.</p>
              ) : (
                <PlanSteps steps={plan.steps} warnings={plan.warnings} />
              )
            ) : generalError ? null : (
              <div className="flex items-center gap-2 text-body text-secondary">
                <Spinner className="size-3.5" /> Reading your Cloudflare account…
              </div>
            )}
            {plan?.requiresConfirmation && !preview.isPending ? (
              <label htmlFor="route-confirm" className="flex items-center gap-2 text-body">
                <Checkbox
                  id="route-confirm"
                  checked={confirmed}
                  onCheckedChange={(value) => setConfirmed(value === true)}
                />
                {plan.warnings.some((w) => w.type === "publicNetwork")
                  ? "Send these public addresses through this Mac"
                  : "Replace the existing records"}
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
          </div>
        ) : null}

        {stage === "applying" && plan ? (
          <div className="flex flex-col gap-3" aria-live="polite">
            <PlanSteps steps={plan.steps} states={apply.steps} />
            {failed ? (
              <div role="alert" className="flex flex-col gap-1 text-callout">
                <p className="text-error">{failed.error}</p>
                {failed.type === "rolledBack" ? (
                  <p className="text-secondary">
                    Everything done before the failure was undone. Nothing was left behind.
                  </p>
                ) : (
                  <>
                    <p className="text-secondary">Some changes couldn't be undone:</p>
                    <ul className="list-disc pl-5 text-secondary">
                      {failed.leftovers.map((l) => (
                        <li key={l}>{l}</li>
                      ))}
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
                <Spinner className="size-6" label="Checking" />
                <p className="text-body text-secondary">
                  Checking {url} works… The connector can take a few seconds to connect.
                </p>
              </>
            ) : verify.data.failure ? (
              <>
                <TriangleAlert aria-hidden className="size-7 text-warning" strokeWidth={1.5} />
                <p className="max-w-sm text-body">{verify.data.message}</p>
                <CopyField label="URL" value={url} className="w-full max-w-sm" />
              </>
            ) : verify.data.protected ? (
              <>
                <LockKeyhole aria-hidden className="size-7 text-healthy" strokeWidth={1.5} />
                <p className="text-headline">Protected by a login</p>
                <p className="max-w-sm text-body text-secondary">
                  Cloudflare asks visitors to sign in before they reach your app.
                </p>
                <div className="flex w-full max-w-sm items-center gap-2">
                  <CopyField label="URL" value={url} className="min-w-0 flex-1" />
                  <Button onClick={() => void openUrl(url)}>
                    Open <ExternalLink />
                  </Button>
                </div>
              </>
            ) : (
              <>
                <CircleCheck aria-hidden className="size-7 text-healthy" strokeWidth={1.5} />
                <p className="text-headline">It works</p>
                <div className="flex w-full max-w-sm items-center gap-2">
                  <CopyField label="URL" value={url} className="min-w-0 flex-1" />
                  <Button onClick={() => void openUrl(url)}>
                    Open <ExternalLink />
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
