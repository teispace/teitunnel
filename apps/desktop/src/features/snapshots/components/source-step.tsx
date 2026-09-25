import { FolderOpen, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { type MessageKey, t } from "@/lib/i18n";
import type { Framework, PreparedView, Project, ProjectWarning } from "@/lib/ipc/bindings";
import { formatBytes } from "../format";

export type SourceKind = "folder" | "build" | "site" | "keep";

export interface SourceState {
  kind: SourceKind;
  folder: string | null;
  project: Project | null;
  url: string;
}

const frameworks: Record<Framework, string> = {
  vite: "Vite",
  next: "Next.js",
  astro: "Astro",
  svelteKit: "SvelteKit",
  nuxt: "Nuxt",
  createReactApp: "Create React App",
  gatsby: "Gatsby",
  docusaurus: "Docusaurus",
  vitePress: "VitePress",
  angular: "Angular",
  other: "",
  static: "",
};

const warnings: Record<ProjectWarning, MessageKey> = {
  nextNeedsExport: "snapshots.source.nextNeedsExport",
  svelteKitNeedsStaticAdapter: "snapshots.source.svelteKitNeedsStaticAdapter",
};

/** What the build of a project runs, as shown before it runs. */
export function buildCommand(project: Project): string | null {
  return project.manager && project.script ? `${project.manager} run ${project.script}` : null;
}

function projectSummary(project: Project): string {
  const name = frameworks[project.framework];
  const command = buildCommand(project);
  if (!command) return t("snapshots.source.staticSite");
  return name
    ? t("snapshots.source.detected", { framework: name, command, output: project.output })
    : t("snapshots.source.detectedOther", { command, output: project.output });
}

interface SourceStepProps {
  state: SourceState;
  onChange: (next: SourceState) => void;
  /** Update mode: "keep the live files" is offered. */
  canKeep: boolean;
  onChooseFolder: () => void;
  detecting: boolean;
  prepared: PreparedView | null;
  /** A build's output so far. */
  output: readonly string[];
  error: string | null;
}

/** Where the files come from: a folder, a project's build, or a running site. */
export function SourceStep({
  state,
  onChange,
  canKeep,
  onChooseFolder,
  detecting,
  prepared,
  output,
  error,
}: SourceStepProps) {
  const segments = [
    ...(canKeep ? [{ value: "keep" as const, label: t("snapshots.source.keep") }] : []),
    { value: "folder" as const, label: t("snapshots.source.folder") },
    { value: "build" as const, label: t("snapshots.source.project") },
    { value: "site" as const, label: t("snapshots.source.site") },
  ];
  const chosen = state.kind === "build" ? state.project?.dir : state.folder;
  return (
    <div className="flex flex-col gap-4">
      <SegmentedControl
        label={t("snapshots.source.label")}
        segments={segments}
        value={state.kind}
        onValueChange={(kind) => onChange({ ...state, kind })}
        className="self-start"
      />
      {state.kind === "keep" ? (
        <p className="text-callout text-secondary">{t("snapshots.source.keepHelp")}</p>
      ) : null}
      {state.kind === "folder" || state.kind === "build" ? (
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <Button onClick={onChooseFolder} pending={detecting}>
              <FolderOpen />
              {t("snapshots.source.chooseFolder")}
            </Button>
            <span className="selectable min-w-0 flex-1 truncate font-mono text-mono text-secondary">
              {chosen ?? t("snapshots.source.noFolder")}
            </span>
          </div>
          <p className="text-callout text-secondary">
            {state.kind === "folder"
              ? t("snapshots.source.folderHelp")
              : state.project
                ? projectSummary(state.project)
                : t("snapshots.source.projectHelp")}
          </p>
          {state.kind === "build" && state.project?.warning ? (
            <p className="flex gap-2 rounded-card bg-warning/10 px-3 py-2 text-callout">
              <TriangleAlert aria-hidden className="mt-0.5 size-3.5 shrink-0 text-warning" />
              <span>{t(warnings[state.project.warning])}</span>
            </p>
          ) : null}
        </div>
      ) : null}
      {state.kind === "site" ? (
        <Field label={t("snapshots.source.url")} help={t("snapshots.source.urlHelp")}>
          {(control) => (
            <Input
              {...control}
              spellCheck={false}
              autoComplete="off"
              placeholder="http://localhost:5173"
              value={state.url}
              onChange={(event) => onChange({ ...state, url: event.target.value })}
              className="font-mono text-mono"
            />
          )}
        </Field>
      ) : null}
      {output.length > 0 ? (
        <pre
          role="log"
          aria-label={t("snapshots.source.buildOutput")}
          className="selectable max-h-40 overflow-auto rounded-card bg-surface-inset px-3 py-2 font-mono text-mono text-secondary"
        >
          {output.join("\n")}
        </pre>
      ) : null}
      {prepared ? (
        <div className="flex flex-col gap-1 text-callout">
          <p>
            {t("snapshots.prepared.summary", {
              files: t("snapshots.files", { count: prepared.files ?? 0 }),
              size: formatBytes(prepared.bytes ?? 0),
            })}
          </p>
          {prepared.skipped.length > 0 ? (
            <p className="text-secondary">
              {t("snapshots.prepared.skipped", {
                count: prepared.skipped.length,
                names: prepared.skipped
                  .slice(0, 4)
                  .map((s) => s.path)
                  .join(", "),
              })}
            </p>
          ) : null}
          {prepared.crawl?.truncated ? (
            <p className="text-warning">{t("snapshots.prepared.truncated")}</p>
          ) : null}
          {prepared.crawl && prepared.crawl.failed.length > 0 ? (
            <p className="text-secondary">
              {t("snapshots.prepared.failed", { count: prepared.crawl.failed.length })}
            </p>
          ) : null}
        </div>
      ) : null}
      {error ? (
        <p role="alert" className="selectable whitespace-pre-wrap text-callout text-error">
          {error}
        </p>
      ) : null}
    </div>
  );
}
