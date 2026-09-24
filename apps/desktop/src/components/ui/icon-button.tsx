import { cva, type VariantProps } from "class-variance-authority";
import type { LucideIcon } from "lucide-react";
import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";
import { Spinner } from "./spinner";

const iconButtonVariants = cva(
  [
    "inline-flex shrink-0 items-center justify-center rounded-full text-secondary outline-offset-1",
    "transition-[background-color,color] transition-snappy disabled:pointer-events-none disabled:opacity-40 aria-busy:opacity-100",
  ],
  {
    variants: {
      variant: {
        plain: "bg-transparent active:bg-surface-control active:text-primary",
        secondary: "bg-surface-control active:bg-surface-control-pressed",
      },
      size: {
        sm: "size-5 [&_svg]:size-3.5",
        md: "size-6 [&_svg]:size-4",
        lg: "size-7 [&_svg]:size-4",
      },
    },
    defaultVariants: { variant: "plain", size: "md" },
  },
);

interface IconButtonProps
  extends Omit<ComponentProps<"button">, "children">,
    VariantProps<typeof iconButtonVariants> {
  icon: LucideIcon;
  /** Required: icon-only controls need an accessible name (DESIGN §11). */
  label: string;
  /** Busy with what it started: disabled, the icon replaced by a spinner. */
  pending?: boolean;
}

export function IconButton({
  icon: Icon,
  label,
  variant,
  size,
  className,
  type = "button",
  pending = false,
  disabled,
  ...props
}: IconButtonProps) {
  return (
    <button
      type={type}
      aria-label={label}
      title={label}
      disabled={disabled || pending}
      aria-busy={pending || undefined}
      className={cn(iconButtonVariants({ variant, size }), className)}
      {...props}
    >
      {pending ? <Spinner label={null} /> : <Icon aria-hidden strokeWidth={1.75} />}
    </button>
  );
}
