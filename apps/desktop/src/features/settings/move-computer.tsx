import { useState } from "react";
import { toast } from "sonner";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import { type MessageKey, t } from "@/lib/i18n";
import type { BackupSummary } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import {
  chooseBackupOpen,
  chooseBackupSave,
  useCreateBackup,
  useInspectBackup,
  useRestoreBackup,
} from "./queries";

const MIN_PASSPHRASE = 10;

const sectionLabels: Record<string, MessageKey> = {
  settings: "backup.section.settings",
  local_tunnels: "backup.section.local_tunnels",
  dns_ownership: "backup.section.dns_ownership",
  access_ownership: "backup.section.access_ownership",
  balanced_routes: "backup.section.balanced_routes",
  snapshots: "backup.section.snapshots",
  snapshot_versions: "backup.section.snapshot_versions",
};

function sectionName(section: string): string {
  const label = sectionLabels[section];
  return label ? t(label) : section;
}

type Mode = { kind: "export"; path: string } | { kind: "import"; path: string };

function Summary({ summary }: { summary: BackupSummary }) {
  return (
    <div className="flex flex-col gap-2">
      <p className="text-body">
        {t("backup.from", { machine: summary.machine, version: summary.appVersion })}
      </p>
      <ul className="flex flex-col rounded-card bg-surface-inset px-3 py-1 text-body">
        {summary.sections.map((section) => (
          <li
            key={section.section}
            className="flex justify-between gap-3 border-inset border-b-hairline py-1.5 last:border-b-0"
          >
            <span>{sectionName(section.section)}</span>
            <span className="tabular text-secondary">
              {section.count}
              {section.existing > 0 && section.section !== "settings"
                ? ` · ${t("backup.replaces", { count: section.existing })}`
                : ""}
            </span>
          </li>
        ))}
      </ul>
      {summary.projects.length > 0 ? (
        <p className="text-callout text-secondary">
          {t("backup.projects", { names: summary.projects.join(", ") })}
        </p>
      ) : null}
      {summary.accounts.length > 0 ? (
        <p className="text-callout">
          {t("backup.accounts", { names: summary.accounts.map((a) => a.name).join(", ") })}
        </p>
      ) : null}
      {summary.overwrites ? (
        <p role="alert" className="text-callout text-warning">
          {t("backup.overwrites")}
        </p>
      ) : null}
    </div>
  );
}

/** Settings ▸ General ▸ Move to another computer: export and import an encrypted backup. */
export function MoveComputer() {
  const [mode, setMode] = useState<Mode | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [again, setAgain] = useState("");
  const create = useCreateBackup();
  const inspect = useInspectBackup();
  const restore = useRestoreBackup();
  const close = () => {
    setMode(null);
    setPassphrase("");
    setAgain("");
    create.reset();
    inspect.reset();
    restore.reset();
  };
  const failure = create.error ?? inspect.error ?? restore.error;
  const error = failure ? toIpcError(failure) : null;
  const tooShort = passphrase.length > 0 && passphrase.length < MIN_PASSPHRASE;
  const mismatch = mode?.kind === "export" && again.length > 0 && again !== passphrase;
  const summary = inspect.data?.summary ?? null;

  const save = () => {
    if (mode?.kind !== "export") return;
    create.mutate(
      { path: mode.path, passphrase },
      {
        onSuccess: () => {
          toast.success(t("backup.saved", { path: mode.path }));
          close();
        },
      },
    );
  };
  const read = () => {
    if (mode?.kind === "import") inspect.mutate({ path: mode.path, passphrase });
  };
  const apply = () => {
    if (!inspect.data) return;
    restore.mutate(inspect.data.id, {
      onSuccess: () => {
        toast.success(t("backup.restored"));
        close();
      },
    });
  };

  const footer =
    mode?.kind === "export" ? (
      <>
        <Button onClick={close}>{t("common.cancel")}</Button>
        <Button
          variant="primary"
          pending={create.isPending}
          disabled={passphrase.length < MIN_PASSPHRASE || again !== passphrase}
          onClick={save}
        >
          {t("backup.save")}
        </Button>
      </>
    ) : (
      <>
        <Button onClick={close}>{t("common.cancel")}</Button>
        {summary ? (
          <Button variant="primary" pending={restore.isPending} onClick={apply}>
            {summary.overwrites ? t("backup.restoreReplace") : t("backup.restore")}
          </Button>
        ) : (
          <Button
            variant="primary"
            pending={inspect.isPending}
            disabled={passphrase.length === 0}
            onClick={read}
          >
            {t("backup.read")}
          </Button>
        )}
      </>
    );

  return (
    <GroupedSection title={t("backup.title")} footer={t("backup.description")}>
      <GroupedRow label={t("backup.rowLabel")} description={t("backup.rowDescription")}>
        <div className="flex gap-2">
          <Button
            size="sm"
            onClick={() =>
              void chooseBackupSave().then((path) => path && setMode({ kind: "export", path }))
            }
          >
            {t("backup.export")}
          </Button>
          <Button
            size="sm"
            onClick={() =>
              void chooseBackupOpen().then((path) => path && setMode({ kind: "import", path }))
            }
          >
            {t("backup.import")}
          </Button>
        </div>
      </GroupedRow>
      <Sheet open={mode !== null} onOpenChange={(open) => !open && close()}>
        {mode ? (
          <SheetContent title={t("backup.title")} description={mode.path} footer={footer}>
            <form
              className="flex flex-col gap-3"
              onSubmit={(event) => {
                event.preventDefault();
                if (mode.kind === "export") save();
                else if (!summary) read();
              }}
            >
              {summary ? null : (
                <Field
                  label={t("backup.passphrase")}
                  help={mode.kind === "export" ? t("backup.passphraseHelp") : undefined}
                  error={tooShort && mode.kind === "export" ? t("backup.tooShort") : null}
                >
                  {(control) => (
                    <Input
                      {...control}
                      type="password"
                      autoComplete="new-password"
                      value={passphrase}
                      onChange={(event) => setPassphrase(event.target.value)}
                    />
                  )}
                </Field>
              )}
              {mode.kind === "export" ? (
                <Field
                  label={t("backup.passphraseAgain")}
                  error={mismatch ? t("backup.mismatch") : null}
                >
                  {(control) => (
                    <Input
                      {...control}
                      type="password"
                      autoComplete="new-password"
                      value={again}
                      onChange={(event) => setAgain(event.target.value)}
                    />
                  )}
                </Field>
              ) : null}
              {summary ? <Summary summary={summary} /> : null}
              {error ? (
                <p role="alert" className="text-callout text-error">
                  {error.message}
                </p>
              ) : null}
            </form>
          </SheetContent>
        ) : null}
      </Sheet>
    </GroupedSection>
  );
}
