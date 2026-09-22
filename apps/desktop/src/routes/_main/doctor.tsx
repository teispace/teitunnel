import { createFileRoute } from "@tanstack/react-router";
import { Stethoscope } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";

export const Route = createFileRoute("/_main/doctor")({
  component: DoctorPage,
});

function DoctorPage() {
  return (
    <>
      <TitlebarToolbar title="Doctor" />
      <EmptyState
        icon={Stethoscope}
        title="No issues found"
        description="Doctor checks DNS, tunnels and local services and suggests fixes."
      />
    </>
  );
}
