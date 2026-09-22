import type { ReactNode } from "react";

/**
 * The unified 52 px title bar + toolbar band above the content pane. The whole band is a
 * window drag region; buttons and inputs inside it stay interactive.
 */
export function TitlebarToolbar({ title, children }: { title: ReactNode; children?: ReactNode }) {
  return (
    <header
      data-tauri-drag-region="deep"
      className="flex h-(--toolbar-height) shrink-0 items-center gap-3 pr-3 pl-5"
    >
      <h1 className="min-w-0 truncate text-title3">{title}</h1>
      {children ? <div className="ml-auto flex items-center gap-2">{children}</div> : null}
    </header>
  );
}
