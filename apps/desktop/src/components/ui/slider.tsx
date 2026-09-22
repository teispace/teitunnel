import { Slider as SliderPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";

export function Slider({ className, ...props }: ComponentProps<typeof SliderPrimitive.Root>) {
  const thumbs = (props.value ?? props.defaultValue ?? [0]).length;
  return (
    <SliderPrimitive.Root
      className={cn(
        "relative flex h-5 w-full touch-none select-none items-center disabled:opacity-40",
        className,
      )}
      {...props}
    >
      <SliderPrimitive.Track className="relative h-1 grow overflow-hidden rounded-full bg-surface-control-pressed">
        <SliderPrimitive.Range className="absolute h-full bg-accent" />
      </SliderPrimitive.Track>
      {Array.from({ length: thumbs }, (_, i) => (
        <SliderPrimitive.Thumb
          // biome-ignore lint/suspicious/noArrayIndexKey: thumbs are positional
          key={i}
          className="block h-3.5 w-5 rounded-full bg-surface-knob shadow-[0_1px_3px_rgb(0_0_0/0.3),0_0_0_0.5px_rgb(0_0_0/0.08)] outline-offset-1"
        />
      ))}
    </SliderPrimitive.Root>
  );
}
