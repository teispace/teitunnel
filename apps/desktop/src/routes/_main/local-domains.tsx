import { createFileRoute } from "@tanstack/react-router";
import { LocalDomainsPage } from "@/features/local-domains";

export const Route = createFileRoute("/_main/local-domains")({
  component: LocalDomainsPage,
});
