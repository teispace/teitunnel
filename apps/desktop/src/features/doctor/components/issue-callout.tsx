import { useRouter } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-dot";
import { t, translate } from "@/lib/i18n";
import type { Issue } from "@/lib/ipc/bindings";

/** A Doctor issue shown where it applies (a route's or tunnel's inspector). */
export function IssueCallout({ issue }: { issue: Issue }) {
  // No router around it (isolated renders): the button just does nothing.
  const router = useRouter({ warn: false }) as ReturnType<typeof useRouter> | undefined;
  return (
    <section
      aria-label={translate(issue.title)}
      className="flex items-start gap-2.5 rounded-row bg-surface-pressed px-3 py-2.5"
    >
      <StatusDot
        status={issue.severity === "error" ? "error" : "warning"}
        label={t(`doctor.severity.${issue.severity}.label`)}
        className="mt-1"
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="text-body font-medium">{translate(issue.title)}</p>
        <p className="text-callout text-secondary">{translate(issue.detail)}</p>
      </div>
      <Button
        size="sm"
        onClick={() => void router?.navigate({ to: "/doctor", search: { issue: issue.id } })}
      >
        {issue.fixes.length > 0 ? t("doctor.inline.fix") : t("doctor.inline.show")}
      </Button>
    </section>
  );
}
