import { createFileRoute } from "@tanstack/react-router";
import { DoctorPage } from "@/features/doctor";

export const Route = createFileRoute("/_main/doctor")({
  component: DoctorPage,
});
