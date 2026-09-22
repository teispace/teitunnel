import { useIssues } from "./queries";

/** The number of errors, next to Doctor in the sidebar (warnings don't count). */
export function DoctorBadge() {
  const { issues } = useIssues();
  const errors = issues.filter((issue) => issue.severity === "error").length;
  if (errors === 0) return null;
  return (
    <span
      role="status"
      aria-label={`${errors} problem${errors === 1 ? "" : "s"}`}
      className="flex h-4 min-w-4 items-center justify-center rounded-full bg-error px-1 text-footnote font-semibold text-on-accent tabular group-data-[status=active]:bg-on-accent group-data-[status=active]:text-accent"
    >
      {errors}
    </span>
  );
}
