import { createFileRoute, Outlet } from "@tanstack/react-router";
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
        </Sidebar>
      }
    >
      <Outlet />
    </AppShell>
  );
}
