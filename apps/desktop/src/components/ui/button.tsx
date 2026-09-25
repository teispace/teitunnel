import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";
import { Spinner } from "./spinner";

/**
 * macOS 27 push button: a flat capsule fill, arrow cursor, instant press feedback,
 * no scale-down (DESIGN §7). Primary is the default button (accent fill).
 */
export const buttonVariants = cva(
  [
    "inline-flex shrink-0 select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-full",
    "font-normal outline-offset-1 transition-[background-color,color,opacity] transition-snappy",
    "disabled:pointer-events-none disabled:opacity-40 aria-busy:opacity-70",
    // While busy, the spinner stands in for the button's own icon.
    "aria-busy:[&>svg:not([data-spinner])]:hidden",
    "[&_svg]:pointer-events-none [&_svg]:shrink-0",
  ],
  {
    variants: {
      variant: {
        primary:
          "bg-accent-fill text-on-accent active:brightness-90 [:root[data-window-active=false]_&]:bg-surface-control [:root[data-window-active=false]_&]:text-primary",
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
  /**
   * Busy with what it started: disabled, with a spinner in place of its icon until the
   * result is on screen (DESIGN §7). Not for `asChild`.
   */
  pending?: boolean;
}

export function Button({
  className,
  variant,
  size,
  asChild = false,
  pending = false,
  type = "button",
  children,
  ...props
}: ButtonProps) {
  if (asChild) {
    return (
      <Slot.Root className={cn(buttonVariants({ variant, size }), className)} {...props}>
        {children}
      </Slot.Root>
    );
  }
  return (
    <button
      type={type}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
      disabled={props.disabled || pending}
      aria-busy={pending || undefined}
    >
      {pending ? <Spinner className="text-current" label={null} /> : null}
      {children}
    </button>
  );
}
