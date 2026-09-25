import { Folder, FolderOpen, X } from "lucide-react";
import { useId, useState } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { IconButton } from "@/components/ui/icon-button";
import { Tooltip } from "@/components/ui/tooltip";
import { t } from "@/lib/i18n";
import type { FolderShare } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { chooseFolder, resolveFolder } from "../queries";

/** The folder being shared instead of a service: its name, and a way back to services. */
export function FolderChip({ folder, onClear }: { folder: FolderShare; onClear: () => void }) {
  const name = folder.path.split(/[\\/]/).filter(Boolean).pop() ?? folder.path;
  return (
    <div className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-control bg-surface-content pr-0.5 pl-2">
      <Folder aria-hidden className="size-3.5 shrink-0 text-accent" strokeWidth={2} />
      <span className="truncate text-body text-primary" title={folder.path}>
        {name}
      </span>
      <span className="min-w-0 truncate text-callout text-tertiary">{folder.path}</span>
      <IconButton
        icon={X}
        label={t("quickShare.folder.clear")}
        size="sm"
        className="ml-auto"
        onClick={onClear}
      />
    </div>
  );
}

/** Opens the folder panel; the chosen folder is checked before it's used. */
export function ChooseFolderButton({
  onChosen,
  onError,
}: {
  onChosen: (folder: FolderShare) => void;
  onError: (message: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const choose = async () => {
    setBusy(true);
    try {
      const path = await chooseFolder();
      if (path) onChosen(await resolveFolder(path));
    } catch (error) {
      onError(toIpcError(error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Tooltip content={t("quickShare.folder.choose")}>
      <IconButton
        icon={FolderOpen}
        label={t("quickShare.folder.choose")}
        variant="secondary"
        size="lg"
        pending={busy}
        onClick={() => void choose()}
      />
    </Tooltip>
  );
}

/** Folder options: a file listing, and the single-page-app fallback. */
export function FolderOptions({
  folder,
  onChange,
}: {
  folder: FolderShare;
  onChange: (folder: FolderShare) => void;
}) {
  const listingId = useId();
  const spaId = useId();
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={listingId} className="flex items-center gap-2 text-body">
        <Checkbox
          id={listingId}
          checked={folder.listing ?? false}
          onCheckedChange={(checked) => onChange({ ...folder, listing: checked === true })}
        />
        {t("quickShare.folder.listing")}
      </label>
      <label htmlFor={spaId} className="flex items-center gap-2 text-body">
        <Checkbox
          id={spaId}
          checked={folder.spa ?? false}
          onCheckedChange={(checked) => onChange({ ...folder, spa: checked === true })}
        />
        {t("quickShare.folder.spa")}
      </label>
      <p className="text-footnote text-secondary">{t("quickShare.folder.safety")}</p>
    </div>
  );
}
