import { Link, useNavigate } from "@tanstack/react-router";
import type { LucideIcon } from "lucide-react";
import { PanelLeft } from "lucide-react";
import type { KeyboardEvent, ReactNode } from "react";
import type { NavItem } from "@/app/navigation";
import { useUiStore } from "@/app/ui-store";
import { IconButton } from "@/components/ui/icon-button";
import { cn } from "@/lib/cn";

/** Full-height source-list sidebar over the window's native vibrancy (DESIGN §2). */
export function Sidebar({ children, footer }: { children: ReactNode; footer?: ReactNode }) {
  return (
    <aside
      aria-label="Sidebar"
      className="flex h-full w-(--sidebar-width) shrink-0 flex-col bg-surface-sidebar"
    >
      <div
        data-tauri-drag-region="deep"
        className="flex h-(--toolbar-height) shrink-0 items-center justify-end px-2.5"
      >
        <IconButton
          icon={PanelLeft}
          label="Hide sidebar"
          onClick={useUiStore.getState().toggleSidebar}
        />
      </div>
      <nav
        onKeyDown={moveSelection}
        className="flex-1 overflow-y-auto overscroll-contain px-2.5 pb-3"
      >
        {children}
      </nav>
      {footer ? <div className="shrink-0 px-2.5 pb-2.5">{footer}</div> : null}
    </aside>
  );
}

/** Up/Down move between items and select them, as in a native source list. */
function moveSelection(event: KeyboardEvent<HTMLElement>) {
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  const links = [...event.currentTarget.querySelectorAll<HTMLAnchorElement>("a[href]")];
  const current = links.indexOf(document.activeElement as HTMLAnchorElement);
  const next = links[current + (event.key === "ArrowDown" ? 1 : -1)];
  if (!next) return;
  event.preventDefault();
  next.focus();
  next.click();
}

export function SidebarSection({ title, children }: { title: string | null; children: ReactNode }) {
  return (
    <section aria-label={title ?? undefined} className="mt-3.5 first:mt-0">
      {title ? (
        <h2 className="px-2 pt-1 pb-1.5 text-footnote font-semibold text-tertiary">{title}</h2>
      ) : null}
      <ul className="flex flex-col">{children}</ul>
    </section>
  );
}

interface SidebarItemProps {
  to: NavItem["to"] | "/dev/gallery";
  label: string;
  icon: LucideIcon;
  badge?: ReactNode;
  exact?: boolean;
}

/**
 * A sidebar row. Like NSOutlineView, selection happens on mouse-down, not on click,
 * which is what makes native sidebars feel instant.
 */
export function SidebarItem({ to, label, icon: Icon, badge, exact = false }: SidebarItemProps) {
  const navigate = useNavigate();
  return (
    <li>
      <Link
        to={to}
        activeOptions={{ exact }}
        draggable={false}
        onMouseDown={(event) => {
          if (event.button === 0 && !event.metaKey && !event.shiftKey) void navigate({ to });
        }}
        className={cn(
          "group flex h-8 items-center gap-2.5 rounded-row px-2 text-body text-primary",
          "cursor-default outline-none focus-visible:outline-offset-[-2px]",
          "data-[status=active]:bg-surface-selected data-[status=active]:text-on-accent",
        )}
      >
        <Icon
          aria-hidden
          size={18}
          strokeWidth={1.6}
          className="shrink-0 text-sidebar-icon group-data-[status=active]:text-on-accent"
        />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {badge}
      </Link>
    </li>
  );
}
