import { FileWarning } from "lucide-react";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { type MessageKey, t, translate } from "@/lib/i18n";
import type { ItemKind, ItemState, ProjectItem, ProjectPlan } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useProjectModified, useProjectStatus } from "../queries";

const kindLabels: Record<ItemKind, MessageKey> = {
  route: "project.kind.route",
  share: "project.kind.share",
  snapshot: "project.kind.snapshot",
  localDomain: "project.kind.localDomain",
};

const stateLabels: Record<ItemState, MessageKey> = {
  applied: "project.state.applied",
  differs: "project.state.differs",
  missing: "project.state.missing",
  unsupported: "project.state.unsupported",
};

const stateTones = {
  applied: "healthy",
  differs: "warning",
  missing: "neutral",
  unsupported: "neutral",
} as const satisfies Record<ItemState, "healthy" | "warning" | "neutral">;

function ItemRow({ item }: { item: ProjectItem }) {
  const name =
    item.kind === "share" && item.name === "trycloudflare.com"
      ? t("project.randomAddress")
      : item.name;
  return (
    <GroupedRow
      label={
        <span className="flex min-w-0 flex-col">
          <span className="truncate">
            <span className="text-secondary">{t(kindLabels[item.kind])} · </span>
            <span className="font-mono text-mono">{name}</span>
          </span>
          <span className="truncate text-footnote text-secondary">
            {item.target} · {t("project.line", { line: item.line })}
            {item.note ? ` · ${translate(item.note)}` : ""}
          </span>
        </span>
      }
    >
      <Badge tone={stateTones[item.state]}>{t(stateLabels[item.state])}</Badge>
    </GroupedRow>
  );
}

interface ProjectDetailProps {
  path: string;
  onApply: (plan: ProjectPlan) => void;
  onRemove: () => void;
}

/**
 * One project: the file's problems (with their lines), each declared item and whether
 * it's applied, and Apply. Edits to the file are noticed and offered for review.
 */
export function ProjectDetail({ path, onApply, onRemove }: ProjectDetailProps) {
  const status = useProjectStatus(path);
  const modified = useProjectModified(path);
  if (status.isPending) {
    return (
      <div className="flex flex-col gap-4 p-5">
        <SkeletonSection rows={3} />
      </div>
    );
  }
  if (status.error) {
    const error = toIpcError(status.error);
    return (
      <div className="flex flex-col gap-2 p-5">
        <p role="alert" className="text-body text-error">
          {t("project.loadFailed")}: {error.message}
        </p>
        <div>
          <Button onClick={onRemove}>{t("project.remove")}</Button>
        </div>
      </div>
    );
  }
  const { data } = status;
  const plan = data.plan;
  const changed =
    modified.data !== undefined &&
    modified.data !== null &&
    data.modifiedAt !== null &&
    modified.data !== data.modifiedAt;
  const errors = data.diagnostics.filter((d) => d.severity === "error");
  const warnings = data.diagnostics.filter((d) => d.severity === "warning");
  const empty =
    plan !== null && plan.routes.length + plan.shares.length + plan.snapshots.length === 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-5">
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-title3">{data.name}</h2>
          <p className="truncate font-mono text-mono text-secondary">{data.path}</p>
        </div>
        <div className="flex shrink-0 gap-2">
          <Button onClick={onRemove}>{t("project.remove")}</Button>
          <Button
            variant="primary"
            disabled={plan === null || empty}
            onClick={() => plan && onApply(plan)}
          >
            {t("project.apply")}
          </Button>
        </div>
      </header>

      {changed ? (
        <section
          role="status"
          className="flex items-center justify-between gap-3 rounded-card bg-accent-fill/10 px-3 py-2"
        >
          <span className="text-callout">
            <span className="font-semibold">{t("project.changed.title")}</span>{" "}
            <span className="text-secondary">{t("project.changed.description")}</span>
          </span>
          <Button
            size="sm"
            pending={status.isFetching}
            onClick={() =>
              void status.refetch().then((next) => {
                const again = next.data?.plan;
                if (again && again.routes.length + again.shares.length + again.snapshots.length > 0)
                  onApply(again);
              })
            }
          >
            {t("project.changed.review")}
          </Button>
        </section>
      ) : null}

      {errors.length + warnings.length > 0 ? (
        <GroupedSection
          title={
            errors.length > 0
              ? t("project.problems", { count: errors.length })
              : t("project.warnings", { count: warnings.length })
          }
        >
          {[...errors, ...warnings].map((d) => (
            <GroupedRow
              key={`${d.line}:${d.column}:${d.message.key}`}
              label={
                <span className="flex items-start gap-2">
                  <FileWarning
                    aria-hidden
                    className={
                      d.severity === "error"
                        ? "mt-0.5 size-3.5 shrink-0 text-error"
                        : "mt-0.5 size-3.5 shrink-0 text-warning"
                    }
                    strokeWidth={1.75}
                  />
                  <span className="flex flex-col">
                    <span>{translate(d.message)}</span>
                    <span className="text-footnote text-secondary">
                      {t("project.position", { line: d.line, column: d.column })}
                    </span>
                  </span>
                </span>
              }
            />
          ))}
        </GroupedSection>
      ) : null}

      {data.problem ? (
        <p role="alert" className="text-callout text-error">
          {translate(data.problem)}
        </p>
      ) : null}

      {plan ? (
        <GroupedSection
          title={t("project.items")}
          footer={empty ? t("project.upToDate") : undefined}
        >
          {plan.items.map((item) => (
            <ItemRow key={`${item.kind}:${item.name}:${item.line}`} item={item} />
          ))}
        </GroupedSection>
      ) : null}
    </div>
  );
}
