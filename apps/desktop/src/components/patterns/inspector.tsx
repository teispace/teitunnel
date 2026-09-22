import type { ReactNode } from "react";

interface InspectorProps {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}

/** The right-hand detail pane: a title block, then stacked sections. */
export function Inspector({ title, subtitle, actions, children }: InspectorProps) {
  return (
    <aside aria-label="Inspector" className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className="px-4 pt-3 pb-4">
        <h2 className="selectable truncate text-title2">{title}</h2>
        {subtitle ? <div className="mt-0.5 text-callout text-secondary">{subtitle}</div> : null}
        {actions ? <div className="mt-3 flex flex-wrap gap-2">{actions}</div> : null}
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-4 pb-6">{children}</div>
    </aside>
  );
}

export function InspectorSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex min-w-0 flex-col gap-2">
      <h3 className="text-headline text-secondary">{title}</h3>
      {children}
    </section>
  );
}
