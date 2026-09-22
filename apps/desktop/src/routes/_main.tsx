import { createFileRoute, Outlet } from "@tanstack/react-router";
import { SwatchBook } from "lucide-react";
import { MenuBridge } from "@/app/menu-bridge";
import { navigation } from "@/app/navigation";
import { QuitDialog } from "@/app/quit-dialog";
import { AppShell } from "@/components/patterns/app-shell";
import { CommandPalette } from "@/components/patterns/command-palette";
import { Sidebar, SidebarItem, SidebarSection } from "@/components/patterns/sidebar";
import { DoctorBadge } from "@/features/doctor";

export const Route = createFileRoute("/_main")({
  component: MainLayout,
});

function MainLayout() {
  return (
    <AppShell
      sidebar={
        <Sidebar>
          {navigation.map((section) => (
            <SidebarSection key={section.title ?? "main"} title={section.title}>
              {section.items.map((item) => (
                <SidebarItem
                  key={item.to}
                  to={item.to}
                  label={item.label}
                  icon={item.icon}
                  exact={item.to === "/"}
                  {...(item.to === "/doctor" ? { badge: <DoctorBadge /> } : {})}
                />
              ))}
            </SidebarSection>
          ))}
          {import.meta.env.DEV ? (
            <SidebarSection title="Developer">
              <SidebarItem to="/dev/gallery" label="Gallery" icon={SwatchBook} />
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
