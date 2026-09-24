import { type ReactNode, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogTrigger } from "@/components/ui/dialog";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";

interface ConfirmDialogProps {
  trigger: ReactNode;
  title: string;
  description?: ReactNode;
  confirmLabel: string;
  variant?: "primary" | "destructive";
  /**
   * Does the work. The dialog stays open with its button busy until this settles, then
   * closes, or shows why it failed and lets the user try again or cancel.
   */
  onConfirm: () => Promise<unknown>;
}

/** An alert asking to confirm an action that takes a moment (disconnect, stop, adopt). */
export function ConfirmDialog({
  trigger,
  title,
  description,
  confirmLabel,
  variant = "primary",
  onConfirm,
}: ConfirmDialogProps) {
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirm = async () => {
    setPending(true);
    setError(null);
    try {
      await onConfirm();
      setOpen(false);
    } catch (failure) {
      setError(toIpcError(failure).message);
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        // Escape and clicks outside can't abandon work that's under way.
        if (pending) return;
        setOpen(next);
        setError(null);
      }}
    >
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent
        title={title}
        description={description}
        footer={
          <>
            <DialogClose asChild>
              <Button disabled={pending}>{t("common.cancel")}</Button>
            </DialogClose>
            <Button variant={variant} pending={pending} onClick={() => void confirm()}>
              {confirmLabel}
            </Button>
          </>
        }
      >
        {error ? (
          <p role="alert" className="text-callout text-error">
            {error}
          </p>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
