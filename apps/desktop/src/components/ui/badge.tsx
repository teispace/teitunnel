import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";

const badgeVariants = cva(
  "inline-flex h-4 shrink-0 items-center rounded-full px-1.5 text-footnote font-medium tabular",
  {
    variants: {
      tone: {
        neutral: "bg-surface-control text-secondary",
        accent: "bg-accent-fill text-on-accent",
        healthy: "bg-healthy/15 text-healthy",
        warning: "bg-warning/15 text-warning",
        error: "bg-error/15 text-error",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

export function Badge({
  tone,
  className,
  ...props
}: ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}
