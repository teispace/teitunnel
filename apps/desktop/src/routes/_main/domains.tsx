import { createFileRoute } from "@tanstack/react-router";
import { Globe } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";

export const Route = createFileRoute("/_main/domains")({
  component: DomainsPage,
});

function DomainsPage() {
  return (
    <>
      <TitlebarToolbar title="Domains" />
      <EmptyState
        icon={Globe}
        title="No domains connected"
        description="Connect a Cloudflare account to use your domains for routes."
      />
    </>
  );
}
