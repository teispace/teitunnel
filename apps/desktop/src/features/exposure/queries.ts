import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type ExposureReport } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

/** The findings worth showing: `null` when the check is off or found nothing. */
export function findingsOf(report: ExposureReport | null | undefined): ExposureReport | null {
  return report && report.findings.length > 0 ? report : null;
}

/** The check for a service, as a query (a route's review shows it next to the plan). */
export function useExposure(origin: string | null) {
  return useQuery({
    queryKey: queryKeys.exposure(origin ?? ""),
    queryFn: () => call(commands.exposureCheck(origin ?? "")),
    enabled: origin !== null && origin.trim() !== "",
    staleTime: 30_000,
    retry: false,
  });
}

/**
 * Checks a service before sharing it. With findings, the share waits for "Share Anyway";
 * without (or when the check fails or is off), it goes ahead at once: the check never
 * blocks sharing.
 */
export function useExposureGate() {
  const check = useMutation({
    mutationFn: (origin: string) => call(commands.exposureCheck(origin)),
  });
  const [held, setHeld] = useState<{ report: ExposureReport; proceed: () => void } | null>(null);
  const run = (origin: string, proceed: () => void) => {
    setHeld(null);
    check.mutate(origin, {
      onSuccess: (report) => {
        const found = findingsOf(report);
        if (found) setHeld({ report: found, proceed });
        else proceed();
      },
      onError: proceed,
    });
  };
  return {
    run,
    checking: check.isPending,
    report: held?.report ?? null,
    proceed: () => {
      held?.proceed();
      setHeld(null);
    },
    cancel: () => setHeld(null),
  };
}
