import { Dialog as DialogPrimitive } from "radix-ui";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "@/lib/cn";

export const Dialog = DialogPrimitive.Root;
export const DialogTrigger = DialogPrimitive.Trigger;
export const DialogClose = DialogPrimitive.Close;

function Overlay() {
  return (
    <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-surface-backdrop data-[state=closed]:animate-fade-out data-[state=open]:animate-fade-in" />
  );
}

interface DialogContentProps extends ComponentProps<typeof DialogPrimitive.Content> {
  title: string;
  description?: ReactNode;
  footer?: ReactNode;
}

/**
 * Alert-style dialog, used only for destructive confirmation and plan review
 * (DESIGN §1). Centered, compact, with a stacked title and message.
 */
export function DialogContent({
  title,
  description,
  footer,
  children,
  className,
  ...props
}: DialogContentProps) {
  return (
    <DialogPrimitive.Portal>
      <Overlay />
      <DialogPrimitive.Content
        className={cn(
          "fixed top-1/2 left-1/2 z-50 w-[min(420px,calc(100vw-48px))] -translate-x-1/2 -translate-y-1/2",
          "rounded-sheet bg-surface-raised p-5 shadow-sheet outline-none",
          "data-[state=closed]:animate-pop-out data-[state=open]:animate-sheet-in",
          className,
        )}
        {...props}
      >
        <DialogPrimitive.Title className="text-headline">{title}</DialogPrimitive.Title>
        {description ? (
          <DialogPrimitive.Description className="mt-1.5 text-body text-secondary">
            {description}
          </DialogPrimitive.Description>
        ) : (
          <DialogPrimitive.Description className="sr-only">{title}</DialogPrimitive.Description>
        )}
        {children ? <div className="mt-4">{children}</div> : null}
        {footer ? <div className="mt-5 flex justify-end gap-2">{footer}</div> : null}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}
