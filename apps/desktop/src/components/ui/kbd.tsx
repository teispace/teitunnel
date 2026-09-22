import { cn } from "@/lib/cn";

/** A keyboard shortcut, e.g. `⌘K`, drawn the way macOS menus show them. */
export function Kbd({ keys, className }: { keys: string; className?: string }) {
  return (
    <kbd
      className={cn("font-sans text-body text-tertiary tracking-[0.08em] not-italic", className)}
    >
      {keys}
    </kbd>
  );
}
