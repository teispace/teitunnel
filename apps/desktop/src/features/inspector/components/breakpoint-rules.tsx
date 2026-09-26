import { OctagonPause, Plus, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { t } from "@/lib/i18n";
import type { BreakpointRule } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useConfigureTap, useTaps } from "../queries";

type Stage = "request" | "response" | "both";
const STAGES: readonly Stage[] = ["request", "response", "both"];

const stageOf = (rule: BreakpointRule): Stage =>
  rule.request && rule.response ? "both" : rule.response ? "response" : "request";

const withStage = (rule: BreakpointRule, stage: Stage): BreakpointRule => ({
  ...rule,
  request: stage !== "response",
  response: stage !== "request",
});

/** A pattern matching exactly `path` (a regular expression when it has glob characters). */
export function exactPattern(path: string): string {
  return /[*?]/.test(path) ? `re:^${path.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$` : path;
}

/** The breakpoint list in a tap's settings: method, path and where to hold. */
export function BreakpointRules({
  rules,
  onChange,
}: {
  rules: BreakpointRule[];
  onChange: (rules: BreakpointRule[]) => void;
}) {
  const update = (index: number, rule: BreakpointRule) =>
    onChange(rules.map((current, at) => (at === index ? rule : current)));
  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <h3 className="text-headline">{t("inspector.tap.breakpoints.title")}</h3>
        <Button
          size="sm"
          className="ml-auto"
          onClick={() =>
            onChange([...rules, { method: null, path: "/*", request: true, response: false }])
          }
        >
          <Plus /> {t("inspector.tap.breakpoints.add")}
        </Button>
      </div>
      <p className="-mt-1 text-callout text-secondary">{t("inspector.tap.breakpoints.help")}</p>
      {rules.length === 0 ? (
        <p className="text-callout text-secondary">{t("inspector.tap.breakpoints.empty")}</p>
      ) : (
        <ol className="flex flex-col gap-1.5">
          {rules.map((rule, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rules are positional
            <li key={index} className="flex items-center gap-2">
              <Input
                aria-label={t("inspector.tap.method")}
                placeholder={t("inspector.tap.anyMethod")}
                className="w-16 font-mono text-mono uppercase"
                value={rule.method ?? ""}
                onChange={(e) => update(index, { ...rule, method: e.target.value.trim() || null })}
              />
              <Input
                aria-label={t("inspector.tap.path")}
                className="min-w-0 flex-1 font-mono text-mono"
                value={rule.path}
                onChange={(e) => update(index, { ...rule, path: e.target.value })}
              />
              <Select
                label={t("inspector.tap.breakpoints.stage")}
                options={STAGES.map((value) => ({
                  value,
                  label: t(`inspector.tap.breakpoints.stages.${value}`),
                }))}
                value={stageOf(rule)}
                onValueChange={(stage) => update(index, withStage(rule, stage))}
                className="w-28"
              />
              <IconButton
                icon={X}
                label={t("inspector.tap.breakpoints.remove")}
                onClick={() => onChange(rules.filter((_, at) => at !== index))}
              />
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

/**
 * Adds a breakpoint holding requests with this method and path, in one click from a
 * captured request. Hidden when its share or route isn't inspected any more or isn't
 * recording.
 */
export function HoldLikeThis({ tap, method, path }: { tap: string; method: string; path: string }) {
  const taps = useTaps();
  const configure = useConfigureTap();
  const view = taps.data?.find((candidate) => candidate.id === tap);
  if (!view?.capturing) return null;
  const pattern = exactPattern(path);
  const exists = view.breakpoints.some(
    (rule) => rule.request && rule.path === pattern && rule.method?.toUpperCase() === method,
  );
  return (
    <Button
      size="sm"
      disabled={exists}
      pending={configure.isPending}
      onClick={() =>
        configure.mutate(
          {
            tap,
            patch: {
              breakpoints: [
                ...view.breakpoints,
                { method, path: pattern, request: true, response: false },
              ],
            },
          },
          {
            onSuccess: () => toast.success(t("inspector.held.added", { method, path })),
            onError: (error) => toast.error(toIpcError(error).message),
          },
        )
      }
    >
      <OctagonPause /> {t("inspector.held.addHere")}
    </Button>
  );
}
