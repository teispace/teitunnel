import { createFileRoute } from "@tanstack/react-router";
import { TunnelsPage } from "@/features/routes";

export const Route = createFileRoute("/_main/tunnels")({
  component: TunnelsPage,
});
