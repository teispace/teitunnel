import { Toaster as Sonner } from "sonner";

/** In-app notices (sonner), restyled to the raised material. */
export function Toaster() {
  return (
    <Sonner
      position="bottom-right"
      gap={8}
      offset={16}
      visibleToasts={3}
      toastOptions={{
        unstyled: true,
        classNames: {
          toast:
            "flex w-[340px] items-start gap-2.5 rounded-sheet p-3 material-panel shadow-raised text-body text-primary",
          title: "text-headline",
          description: "mt-0.5 text-callout text-secondary",
          actionButton:
            "ml-auto h-5 shrink-0 rounded-full bg-surface-control px-2.5 text-callout active:bg-surface-control-pressed",
          cancelButton: "ml-auto h-5 shrink-0 rounded-full px-2.5 text-callout text-secondary",
          icon: "mt-px size-4 shrink-0",
          success: "[&_[data-icon]]:text-healthy",
          error: "[&_[data-icon]]:text-error",
          warning: "[&_[data-icon]]:text-warning",
        },
      }}
    />
  );
}
