import { createFileRoute } from "@tanstack/react-router";
import { DomainsPage } from "@/features/domains";

export const Route = createFileRoute("/_main/domains")({
  component: DomainsPage,
});
