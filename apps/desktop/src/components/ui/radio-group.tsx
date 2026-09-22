import { RadioGroup as RadioGroupPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";

export function RadioGroup({
  className,
  ...props
}: ComponentProps<typeof RadioGroupPrimitive.Root>) {
  return <RadioGroupPrimitive.Root className={cn("flex flex-col gap-2", className)} {...props} />;
}

export function Radio({ className, ...props }: ComponentProps<typeof RadioGroupPrimitive.Item>) {
  return (
    <RadioGroupPrimitive.Item
      className={cn(
        "inline-flex size-3.5 shrink-0 items-center justify-center rounded-full outline-offset-1",
        "border-hairline border-control bg-surface-toggle shadow-control",
        "data-[state=checked]:border-transparent data-[state=checked]:bg-accent disabled:opacity-40",
        className,
      )}
      {...props}
    >
      <RadioGroupPrimitive.Indicator className="block size-1.5 rounded-full bg-surface-knob" />
    </RadioGroupPrimitive.Item>
  );
}
