import { Dialog as SheetPrimitive } from "radix-ui";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "@/lib/cn";

export const Sheet = SheetPrimitive.Root;
export const SheetTrigger = SheetPrimitive.Trigger;
export const SheetClose = SheetPrimitive.Close;

interface SheetContentProps extends ComponentProps<typeof SheetPrimitive.Content> {
  title: string;
  description?: ReactNode;
  footer?: ReactNode;
  width?: "md" | "lg";
}

/**
 * macOS sheet: drops from just below the toolbar, dims the window behind it, and
 * holds a focused task (e.g. review a plan). Esc cancels.
 */
export function SheetContent({
  title,
  description,
  footer,
  width = "md",
  children,
  className,
  ...props
}: SheetContentProps) {
  return (
    <SheetPrimitive.Portal>
      <SheetPrimitive.Overlay className="fixed inset-0 z-40 bg-surface-backdrop data-[state=closed]:animate-fade-out data-[state=open]:animate-fade-in" />
      <SheetPrimitive.Content
        className={cn(
          "fixed top-(--toolbar-height) left-1/2 z-50 flex max-h-[calc(100vh-var(--toolbar-height)-24px)] -translate-x-1/2 flex-col",
          width === "md" ? "w-[min(520px,calc(100vw-48px))]" : "w-[min(720px,calc(100vw-48px))]",
          "rounded-sheet bg-surface-raised shadow-sheet outline-none",
          "data-[state=closed]:animate-sheet-out data-[state=open]:animate-sheet-in",
          className,
        )}
        {...props}
      >
        <div className="px-5 pt-5">
          <SheetPrimitive.Title className="text-title3">{title}</SheetPrimitive.Title>
          {description ? (
            <SheetPrimitive.Description className="mt-1 text-body text-secondary">
              {description}
            </SheetPrimitive.Description>
          ) : (
            <SheetPrimitive.Description className="sr-only">{title}</SheetPrimitive.Description>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer ? (
          <div className="flex justify-end gap-2 border-separator border-t-hairline px-5 py-3">
            {footer}
          </div>
        ) : null}
      </SheetPrimitive.Content>
    </SheetPrimitive.Portal>
  );
}
