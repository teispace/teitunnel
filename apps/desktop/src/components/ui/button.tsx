import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";

/**
 * macOS 27 push button: a flat capsule fill, arrow cursor, instant press feedback,
 * no scale-down (DESIGN §7). Primary is the default button (accent fill).
 */
export const buttonVariants = cva(
  [
    "inline-flex shrink-0 select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-full",
    "font-normal outline-offset-1 transition-[background-color,color,opacity] transition-snappy",
    "disabled:pointer-events-none disabled:opacity-40",
    "[&_svg]:pointer-events-none [&_svg]:shrink-0",
  ],
  {
    variants: {
      variant: {
        primary:
          "bg-accent text-on-accent active:brightness-90 [:root[data-window-active=false]_&]:bg-surface-control [:root[data-window-active=false]_&]:text-primary",
        secondary: "bg-surface-control text-primary active:bg-surface-control-pressed",
        plain: "bg-transparent text-primary active:bg-surface-control",
        destructive: "bg-surface-control text-error active:bg-surface-control-pressed",
      },
      size: {
        sm: "h-5 px-2.5 text-callout [&_svg]:size-3",
        md: "h-6 px-3 text-body [&_svg]:size-3.5",
        lg: "h-7 px-4 text-body [&_svg]:size-4",
      },
    },
    defaultVariants: { variant: "secondary", size: "md" },
  },
);

export interface ButtonProps extends ComponentProps<"button">, VariantProps<typeof buttonVariants> {
  /** Render the child element (e.g. a link) with button styling. */
  asChild?: boolean;
}

export function Button({
  className,
  variant,
  size,
  asChild = false,
  type = "button",
  ...props
}: ButtonProps) {
  const Component = asChild ? Slot.Root : "button";
  return (
    <Component
      type={asChild ? undefined : type}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  );
}
