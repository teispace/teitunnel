import { cn } from "@/lib/cn";

/**
 * Placeholder for content that takes > 300 ms (DESIGN §1). It fades in only after that
 * delay, so fast loads never flash a skeleton.
 */
export function Skeleton({ className }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={cn(
        "rounded-control bg-surface-inset opacity-0 [animation:fade-in_200ms_ease-out_300ms_forwards]",
        className,
      )}
    />
  );
}
