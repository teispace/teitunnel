import { cn } from "@/lib/cn";

/**
 * Placeholder for content that takes > 150 ms (DESIGN §1): the shape of what's coming,
 * with a soft shimmer so it reads as loading, not stuck. It fades in only after that
 * delay, so fast loads never flash a skeleton. Reduce Motion stops the shimmer.
 */
export function Skeleton({ className }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={cn(
        "skeleton rounded-control opacity-0 [animation:fade-in_200ms_ease-out_150ms_forwards]",
        className,
      )}
    />
  );
}
