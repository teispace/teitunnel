import { createFileRoute, Outlet } from "@tanstack/react-router";
import { SwatchBook } from "lucide-react";
import { navigation } from "@/app/navigation";
import { AppShell } from "@/components/patterns/app-shell";
import { Sidebar, SidebarItem, SidebarSection } from "@/components/patterns/sidebar";

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
    </AppShell>
  );
}
