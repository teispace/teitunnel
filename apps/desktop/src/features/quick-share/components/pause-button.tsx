import { Pause, Play } from "lucide-react";
import { IconButton } from "@/components/ui/icon-button";
import { Tooltip } from "@/components/ui/tooltip";
import { t } from "@/lib/i18n";

/** Pauses a share (visitors see a paused page, the address stays) or resumes it. */
export function PauseButton({
  paused,
  pending,
  onToggle,
}: {
  paused: boolean;
  pending: boolean;
  onToggle: () => void;
}) {
  const label = paused ? t("quickShare.pause.resume") : t("quickShare.pause.pause");
  return (
    <Tooltip content={label}>
      <IconButton
        icon={paused ? Play : Pause}
        label={label}
        variant="secondary"
        size="lg"
        pending={pending}
        onClick={onToggle}
      />
    </Tooltip>
  );
}
