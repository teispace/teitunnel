import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { type MessageKey, t, translate } from "@/lib/i18n";
import type { ExposureReport, ExposureSeverity } from "@/lib/ipc/bindings";

const severityLabels: Record<ExposureSeverity, MessageKey> = {
  high: "exposure.severity.high",
  medium: "exposure.severity.medium",
  low: "exposure.severity.low",
};

interface ExposureCalloutProps {
  report: ExposureReport;
  /** The heading: sharing (default) or adding a route. */
  kind?: "share" | "route";
  /** "Share Anyway" / "Cancel", when the action waits for them. */
  actions?: ReactNode;
}

/** What the exposure check found, worst first, with what to do about each. */
export function ExposureCallout({ report, kind = "share", actions }: ExposureCalloutProps) {
  const title = t(kind === "route" ? "exposure.routeTitle" : "exposure.title", {
    origin: report.origin,
  });
  return (
    <section
      role="alert"
      aria-label={title}
      className="flex flex-col gap-2 rounded-card bg-warning/10 px-3 py-2.5"
    >
      <p className="flex items-start gap-2 text-callout font-semibold">
        <TriangleAlert
          aria-hidden
          className="mt-0.5 size-3.5 shrink-0 text-warning"
          strokeWidth={2}
        />
        <span>{title}</span>
      </p>
      <ul className="flex flex-col gap-1.5 pl-5.5">
        {report.findings.map((finding) => (
          <li key={finding.kind} className="flex flex-col text-callout">
            <span>
              <span
                className={cn(
                  "mr-1.5 text-footnote font-semibold",
                  finding.severity === "high" ? "text-error" : "text-secondary",
                )}
              >
                {t(severityLabels[finding.severity])}
              </span>
              {translate(finding.title)}
              {finding.detail ? <span className="text-secondary"> ({finding.detail})</span> : null}{" "}
              <span className="font-mono text-mono text-secondary">
                {t("exposure.path", { path: finding.path })}
              </span>
            </span>
            <span className="text-secondary">{translate(finding.advice)}</span>
          </li>
        ))}
      </ul>
      {report.incomplete ? (
        <p className="pl-5.5 text-footnote text-secondary">{t("exposure.incomplete")}</p>
      ) : null}
      {actions ? <div className="flex justify-end gap-2">{actions}</div> : null}
    </section>
  );
}
