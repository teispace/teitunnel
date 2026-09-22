import { createFileRoute } from "@tanstack/react-router";
import { Network } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";

export const Route = createFileRoute("/_main/tunnels")({
  component: TunnelsPage,
});

function TunnelsPage() {
  return (
    <>
      <TitlebarToolbar title="Tunnels" />
      <EmptyState
        icon={Network}
        title="No tunnels"
        description="Teitunnel creates a tunnel for this Mac when you add your first route."
      />
    </>
  );
}
