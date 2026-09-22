import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";

/** Shared look of single- and multi-line text fields. */
export const fieldClasses = cn(
  "w-full min-w-0 rounded-control border-hairline border-control bg-surface-field px-2 text-body text-primary",
  "placeholder:text-tertiary shadow-[inset_0_0.5px_0_rgb(0_0_0/0.03)]",
  "outline-offset-0 focus:outline-(length:--focus-ring-width) focus:outline-solid focus:outline-(--focus-ring)",
  "disabled:opacity-50 aria-invalid:border-error aria-invalid:focus:outline-error/40",
);

export function Input({ className, type = "text", ...props }: ComponentProps<"input">) {
  return (
    <input
      type={type}
      spellCheck={false}
      autoCorrect="off"
      autoCapitalize="off"
      className={cn(fieldClasses, "h-6", className)}
      {...props}
    />
  );
}
