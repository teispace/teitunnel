import type { ReactNode } from "react";

interface InspectorProps {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}

/** The right-hand detail pane: a title block, then stacked sections (at most 880 px wide). */
export function Inspector({ title, subtitle, actions, children }: InspectorProps) {
  return (
    <aside aria-label="Inspector" className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className="w-full max-w-[880px] px-4 pt-3 pb-4">
        <h2 className="selectable line-clamp-2 break-words text-title2">{title}</h2>
        {subtitle ? <div className="mt-0.5 text-callout text-secondary">{subtitle}</div> : null}
        {actions ? <div className="mt-3 flex flex-wrap gap-2">{actions}</div> : null}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {/* A reading width on a wide window; the scroll bar stays at the pane's edge. */}
        <div className="flex w-full max-w-[880px] flex-col gap-5 px-4 pb-6">{children}</div>
      </div>
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
