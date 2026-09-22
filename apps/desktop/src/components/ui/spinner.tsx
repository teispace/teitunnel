import { cn } from "@/lib/cn";

const SPOKES = 8;

/** The macOS progress spinner: eight fading spokes rotating in steps. */
export function Spinner({ className, label = "Loading" }: { className?: string; label?: string }) {
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox="0 0 16 16"
      className={cn("size-4 animate-spinner text-secondary", className)}
    >
      {Array.from({ length: SPOKES }, (_, i) => (
        <rect
          // biome-ignore lint/suspicious/noArrayIndexKey: spokes are positional
          key={i}
          x="7.25"
          y="1"
          width="1.5"
          height="4"
          rx="0.75"
          fill="currentColor"
          opacity={1 - (i / SPOKES) * 0.85}
          transform={`rotate(${(-i * 360) / SPOKES} 8 8)`}
        />
      ))}
    </svg>
  );
}
