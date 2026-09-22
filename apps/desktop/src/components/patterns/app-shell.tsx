import type { ReactNode } from "react";

/**
 * Window layout: the sidebar sits on the native vibrancy material, the content pane is
 * opaque (DESIGN §2).
 */
export function AppShell({ sidebar, children }: { sidebar: ReactNode; children: ReactNode }) {
  return (
    <div className="flex h-full">
      {sidebar}
      <main className="flex min-w-0 flex-1 flex-col bg-surface-content">{children}</main>
    </div>
  );
}
