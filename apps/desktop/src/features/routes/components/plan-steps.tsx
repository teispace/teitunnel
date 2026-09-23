import {
  Check,
  CircleCheck,
  Copy,
  Globe,
  KeyRound,
  LockKeyhole,
  type LucideIcon,
  Minus,
  Network,
  Power,
  RotateCcw,
  Route as RouteIcon,
  Split,
  TriangleAlert,
  Waypoints,
  X,
} from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/cn";
import { type MessageKey, t, translate } from "@/lib/i18n";
import type { StepKind, StepState, StepView, Warning } from "@/lib/ipc/bindings";

const kindIcons: Record<StepKind, LucideIcon> = {
  createTunnel: Waypoints,
  putConfig: RouteIcon,
  createRecord: Globe,
  updateRecord: Globe,
  deleteRecord: Globe,
  stopConnector: Power,
  deleteTunnel: Waypoints,
  loginMethod: KeyRound,
  accessApp: LockKeyhole,
  networkRoute: Network,
  loadBalancer: Split,
  verify: CircleCheck,
};

function StateIcon({ state }: { state: StepState | undefined }) {
  const common = "size-3.5";
  switch (state?.state) {
    case "running":
    case "undoing":
      return <Spinner className={common} />;
    case "done":
      return <Check aria-hidden className={cn(common, "text-healthy")} strokeWidth={2.5} />;
    case "failed":
    case "undoFailed":
      return <X aria-hidden className={cn(common, "text-error")} strokeWidth={2.5} />;
    case "undone":
      return <RotateCcw aria-hidden className={cn(common, "text-secondary")} strokeWidth={2} />;
    case "skipped":
      return <Minus aria-hidden className={cn(common, "text-tertiary")} strokeWidth={2} />;
    default:
      return (
        <span
          aria-hidden
          className="size-3 rounded-full border-(length:--hairline) border-tertiary"
        />
      );
  }
}

const stateLabels: Record<StepState["state"], MessageKey> = {
  running: "plan.state.running",
  done: "plan.state.done",
  skipped: "plan.state.skipped",
  failed: "plan.state.failed",
  undoing: "plan.state.undoing",
  undone: "plan.state.undone",
  undoFailed: "plan.state.undoFailed",
};

function warningText(warning: Warning): string {
  switch (warning.type) {
    case "replacesForeignRecord":
      return t("plan.warning.replacesForeignRecord", warning);
    case "deletesForeignRecord":
      return t("plan.warning.deletesForeignRecord", warning);
    case "keepsForeignRecord":
      return t("plan.warning.keepsForeignRecord", warning);
    case "tunnelEmpty":
      return t("plan.warning.tunnelEmpty");
    case "remoteOrigin":
      return t("plan.warning.remoteOrigin", warning);
    case "publicNetwork":
      return t("plan.warning.publicNetwork", warning);
    case "overlapsNetwork":
      return t("plan.warning.overlapsNetwork", warning);
    case "singleEndpoint":
      return t("plan.warning.singleEndpoint", warning);
  }
}

interface PlanStepsProps {
  steps: readonly StepView[];
  warnings?: readonly Warning[];
  /** While applying (or afterwards): the state of each step, by index. */
  states?: Record<number, StepState>;
  /** Offer "Copy as command" per step (default: only before applying). */
  copyable?: boolean;
}

/**
 * A plan as an ordered checklist. Before applying, each step shows what it does (and a
 * copy-as-command button); while applying, the leading icon becomes its live state.
 */
export function PlanSteps({ steps, warnings = [], states, copyable = !states }: PlanStepsProps) {
  return (
    <div className="flex flex-col gap-3">
      {warnings.length > 0 ? (
        <ul className="flex flex-col gap-2">
          {warnings.map((warning) => (
            <li
              key={warningText(warning)}
              className="flex gap-2 rounded-card bg-warning/10 px-3 py-2 text-callout"
            >
              <TriangleAlert
                aria-hidden
                className="mt-0.5 size-3.5 shrink-0 text-warning"
                strokeWidth={2}
              />
              <span>{warningText(warning)}</span>
            </li>
          ))}
        </ul>
      ) : null}
      <ol
        aria-label={t("plan.steps")}
        className="flex flex-col rounded-card bg-surface-inset px-3 py-1"
      >
        {steps.map((step, index) => {
          const Icon = kindIcons[step.kind];
          const state = states?.[index];
          return (
            <li
              // Steps are positional; the index is their identity.
              // biome-ignore lint/suspicious/noArrayIndexKey: positional list
              key={index}
              className="flex min-h-8 items-center gap-2.5 border-inset border-b-hairline py-1.5 last:border-b-0"
            >
              <span className="flex size-4 shrink-0 items-center justify-center text-secondary">
                {states ? (
                  <StateIcon state={state} />
                ) : (
                  <Icon aria-hidden className="size-3.5" strokeWidth={1.75} />
                )}
              </span>
              <span
                className={cn(
                  "min-w-0 flex-1 text-body",
                  state?.state === "undone" && "text-secondary line-through",
                )}
              >
                {translate(step.description)}
                {state && (state.state === "failed" || state.state === "undoFailed") ? (
                  <span className="mt-0.5 block text-callout text-error">
                    {translate(state.message)}
                  </span>
                ) : null}
              </span>
              {state ? <span className="sr-only">{t(stateLabels[state.state])}</span> : null}
              {copyable && step.command ? <CopyCommand command={step.command} /> : null}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

function CopyCommand({ command }: { command: string }) {
  return (
    <button
      type="button"
      title={t("plan.copyCommand")}
      aria-label={t("plan.copyCommand")}
      onClick={() => void navigator.clipboard.writeText(command)}
      className="flex size-5 shrink-0 items-center justify-center rounded-full text-tertiary outline-offset-0 active:bg-surface-control active:text-primary"
    >
      <Copy aria-hidden className="size-3" strokeWidth={1.75} />
    </button>
  );
}
