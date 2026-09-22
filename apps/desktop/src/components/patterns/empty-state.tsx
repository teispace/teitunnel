import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

interface EmptyStateProps {
  icon?: LucideIcon;
  title: string;
  description: ReactNode;
  action?: ReactNode;
}

/** One sentence and at most one primary action (DESIGN §8). */
export function EmptyState({ icon: Icon, title, description, action }: EmptyStateProps) {
  return (
    <div className="flex h-full flex-col items-center justify-center px-8 pb-(--toolbar-height) text-center">
      {Icon ? (
        <Icon aria-hidden size={40} strokeWidth={1.25} className="mb-4 text-tertiary" />
      ) : null}
      <h2 className="text-title2">{title}</h2>
      <p className="mt-1.5 max-w-sm text-body text-secondary">{description}</p>
      {action ? <div className="mt-5">{action}</div> : null}
    </div>
  );
}
