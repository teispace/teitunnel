import { createFileRoute } from "@tanstack/react-router";
import { Waypoints } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";

export const Route = createFileRoute("/_main/routes")({
  component: RoutesPage,
});

function RoutesPage() {
  return (
    <>
      <TitlebarToolbar title="Routes" />
      <EmptyState
        icon={Waypoints}
        title="No routes yet"
        description="A route sends a hostname like app.example.com to a service on this Mac, such as localhost:3000."
      />
    </>
  );
}
