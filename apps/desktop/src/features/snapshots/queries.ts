import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useState } from "react";
import { translate } from "@/lib/i18n";
import { commands, type Progress, type SnapshotChange, type StepState } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

/** Snapshots in every account. */
export function useSnapshots() {
  return useQuery({
    queryKey: queryKeys.snapshots.list(),
    queryFn: () => call(commands.snapshotsList()),
    staleTime: 30_000,
  });
}

export function useSnapshotVersions(snapshotId: string | null) {
  return useQuery({
    queryKey: queryKeys.snapshots.versions(snapshotId ?? ""),
    queryFn: () => call(commands.snapshotsVersions(snapshotId ?? "")),
    enabled: snapshotId !== null,
  });
}

/** The system's folder panel; resolves to `null` when cancelled. */
export function chooseFolder() {
  return call(commands.snapshotsChooseFolder());
}

export function useDetectProject() {
  return useMutation({
    mutationFn: (dir: string) => call(commands.snapshotsDetectProject(dir)),
  });
}

export type PrepareVars =
  | { kind: "folder"; path: string }
  | { kind: "build"; dir: string }
  | { kind: "site"; url: string };

/** Collects the files (a folder, a build, a crawl); a build's output streams in. */
export function usePrepare() {
  const [output, setOutput] = useState<string[]>([]);
  const mutation = useMutation({
    mutationFn: (vars: PrepareVars) => {
      setOutput([]);
      switch (vars.kind) {
        case "folder":
          return call(commands.snapshotsPrepareFolder(vars.path));
        case "build": {
          const channel = new Channel<string>();
          channel.onmessage = (line) => setOutput((lines) => [...lines.slice(-199), line]);
          return call(commands.snapshotsPrepareBuild(vars.dir, channel));
        }
        case "site":
          return call(commands.snapshotsPrepareCrawl(vars.url));
      }
    },
  });
  return { ...mutation, output };
}

export function useSnapshotPreview(accountId: string) {
  return useMutation({
    mutationFn: (change: SnapshotChange) => call(commands.snapshotsPreview(accountId, change)),
  });
}

export interface SnapshotApplyVars {
  change: SnapshotChange;
  fingerprint: string;
  confirmed: boolean;
}

/** Applies a reviewed change, tracking each step (and the upload) as it streams in. */
export function useSnapshotApply(accountId: string) {
  const queryClient = useQueryClient();
  const [steps, setSteps] = useState<Record<number, StepState>>({});
  const mutation = useMutation({
    mutationFn: ({ change, fingerprint, confirmed }: SnapshotApplyVars) => {
      setSteps({});
      const channel = new Channel<Progress>();
      channel.onmessage = (progress) =>
        setSteps((current) => ({ ...current, [progress.step]: progress.state }));
      return call(commands.snapshotsApply(accountId, change, fingerprint, confirmed, channel));
    },
    onSettled: () => refresh(queryClient, queryKeys.snapshots.all()),
  });
  return { ...mutation, steps };
}

/** Previews and applies in one go, for changes already confirmed (delete, roll back). */
export function useSnapshotChange() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ accountId, change }: { accountId: string; change: SnapshotChange }) => {
      const plan = await call(commands.snapshotsPreview(accountId, change));
      const outcome = await call(
        commands.snapshotsApply(
          accountId,
          change,
          plan.fingerprint,
          false,
          new Channel<Progress>(),
        ),
      );
      if (outcome.type !== "applied") throw new Error(translate(outcome.error));
      return outcome;
    },
    onSettled: () => refresh(queryClient, queryKeys.snapshots.all()),
  });
}
