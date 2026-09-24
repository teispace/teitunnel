import { FolderGit2, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Skeleton } from "@/components/ui/skeleton";
import { t } from "@/lib/i18n";
import type { ProjectPlan } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useManualRefetch } from "@/lib/use-manual-refetch";
import { ApplySheet } from "./components/apply-sheet";
import { ProjectDetail } from "./components/project-detail";
import { chooseProjectFolder, useAddProject, useProjects, useRemoveProject } from "./queries";

/** Folders with a teitunnel.yml: what each declares, its state, and Apply. */
export function ProjectsPage() {
  const projects = useProjects();
  const add = useAddProject();
  const remove = useRemoveProject();
  const reload = useManualRefetch(projects.refetch);
  const [selected, setSelected] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState<ProjectPlan | null>(null);
  const list = projects.data ?? [];
  const current = list.find((p) => p.path === selected) ?? list[0] ?? null;
  const addError = add.error ? toIpcError(add.error) : null;

  const open = () =>
    void chooseProjectFolder().then((folder) => {
      if (folder) add.mutate(folder, { onSuccess: (entry) => setSelected(entry.path) });
    });

  const toolbar = (
    <TitlebarToolbar title={t("project.title")}>
      <IconButton
        icon={RefreshCw}
        label={t("project.refresh")}
        onClick={reload.refresh}
        pending={reload.refreshing}
      />
      <IconButton icon={Plus} label={t("project.open")} onClick={open} pending={add.isPending} />
    </TitlebarToolbar>
  );
  const addProblem = addError ? (
    <p role="alert" className="px-5 text-callout text-error">
      {addError.message}
    </p>
  ) : null;

  if (projects.error) {
    const error = toIpcError(projects.error);
    return (
      <>
        {toolbar}
        <ErrorState
          title={t("project.loadFailed")}
          message={error.message}
          hint={error.hint}
          action={<Button onClick={() => void projects.refetch()}>{t("common.tryAgain")}</Button>}
        />
      </>
    );
  }

  if (!projects.isPending && list.length === 0) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={FolderGit2}
          title={t("project.empty.title")}
          description={t("project.empty.description")}
          action={
            <Button variant="primary" onClick={open} pending={add.isPending}>
              {t("project.open")}
            </Button>
          }
        />
        {addProblem}
      </>
    );
  }

  return (
    <>
      {toolbar}
      <SplitView
        id="projects"
        list={
          projects.isPending ? (
            <div className="flex flex-col gap-2 p-3">
              <Skeleton className="h-11" />
            </div>
          ) : (
            <ListPane
              label={t("project.list")}
              items={list}
              getId={(project) => project.path}
              selectedId={current?.path ?? null}
              onSelect={setSelected}
              renderRow={(project) => (
                <ListRow
                  title={project.name}
                  subtitle={project.path}
                  leading={
                    <FolderGit2 aria-hidden className="size-4 text-secondary" strokeWidth={1.6} />
                  }
                />
              )}
            />
          )
        }
      >
        {addProblem}
        {current ? (
          <ProjectDetail
            key={current.path}
            path={current.path}
            onApply={setReviewing}
            onRemove={() => remove.mutate(current.path, { onSuccess: () => setSelected(null) })}
          />
        ) : (
          <EmptyState title={t("project.noSelection")} description="" />
        )}
      </SplitView>
      <ApplySheet plan={reviewing} onClose={() => setReviewing(null)} />
    </>
  );
}
