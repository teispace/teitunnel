import { createFileRoute, Outlet } from "@tanstack/react-router";
import { SwatchBook } from "lucide-react";
import { MenuBridge } from "@/app/menu-bridge";
import { navigation } from "@/app/navigation";
import { QuitDialog } from "@/app/quit-dialog";
import { AppShell } from "@/components/patterns/app-shell";
import { CommandPalette } from "@/components/patterns/command-palette";
import { Sidebar, SidebarItem, SidebarSection } from "@/components/patterns/sidebar";
import { DoctorBadge } from "@/features/doctor";
import { UpdateNotice } from "@/features/updates";
import { t } from "@/lib/i18n";

/** The developer section, in dev builds; `?clean` hides it (screenshots for the website). */
const SHOW_DEV =
  import.meta.env.DEV &&
  !(typeof window !== "undefined" && new URLSearchParams(window.location.search).has("clean"));

export const Route = createFileRoute("/_main")({
  component: MainLayout,
});

function MainLayout() {
  return (
    <AppShell
      sidebar={
        <Sidebar footer={<UpdateNotice />}>
          {navigation.map((section) => (
            <SidebarSection
              key={section.title ?? "main"}
              title={section.title ? t(section.title) : null}
            >
              {section.items.map((item) => (
                <SidebarItem
                  key={item.to}
                  to={item.to}
                  label={t(item.label)}
                  icon={item.icon}
                  exact={item.to === "/"}
                  {...(item.to === "/doctor" ? { badge: <DoctorBadge /> } : {})}
                />
              ))}
            </SidebarSection>
          ))}
          {SHOW_DEV ? (
            <SidebarSection title={t("nav.developer")}>
              <SidebarItem to="/dev/gallery" label={t("nav.gallery")} icon={SwatchBook} />
            </SidebarSection>
          ) : null}
        </Sidebar>
      }
    >
      <Outlet />
      <MenuBridge />
      <QuitDialog />
      <CommandPalette />
    </AppShell>
  );
}
