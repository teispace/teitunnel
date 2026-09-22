import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";

interface ErrorStateProps {
  title: string;
  message: string;
  hint?: string | null;
  action?: ReactNode;
}

/** What happened, why, and what to do (DESIGN §10). */
export function ErrorState({ title, message, hint, action }: ErrorStateProps) {
  return (
    <div role="alert" className="flex h-full flex-col items-center justify-center px-8 text-center">
      <TriangleAlert aria-hidden size={36} strokeWidth={1.25} className="mb-3 text-warning" />
      <h2 className="text-title3">{title}</h2>
      <p className="selectable mt-1.5 max-w-sm text-body text-secondary">{message}</p>
      {hint ? <p className="mt-1 max-w-sm text-callout text-secondary">{hint}</p> : null}
      {action ? <div className="mt-4">{action}</div> : null}
    </div>
  );
}
