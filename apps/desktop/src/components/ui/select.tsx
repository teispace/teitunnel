import { Check, ChevronsUpDown } from "lucide-react";
import { Select as SelectPrimitive } from "radix-ui";
import { cn } from "@/lib/cn";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
}

interface SelectProps<T extends string> {
  options: readonly SelectOption<T>[];
  value: T;
  onValueChange: (value: T) => void;
  label: string;
  id?: string;
  disabled?: boolean;
  className?: string;
}

/**
 * NSPopUpButton: the menu opens over the button with the current item aligned to it
 * (Radix `item-aligned`), exactly like macOS pop-up menus.
 */
export function Select<T extends string>({
  options,
  value,
  onValueChange,
  label,
  id,
  disabled,
  className,
}: SelectProps<T>) {
  return (
    <SelectPrimitive.Root
      value={value}
      onValueChange={(next) => onValueChange(next as T)}
      {...(disabled === undefined ? {} : { disabled })}
    >
      <SelectPrimitive.Trigger
        {...(id ? { id } : {})}
        aria-label={label}
        className={cn(
          "inline-flex h-6 min-w-0 items-center gap-1.5 rounded-full bg-surface-control pr-1 pl-3 text-body text-primary outline-offset-1",
          "active:bg-surface-control-pressed disabled:opacity-40",
          className,
        )}
      >
        <span className="min-w-0 flex-1 truncate text-left">
          <SelectPrimitive.Value />
        </span>
        <SelectPrimitive.Icon className="flex size-4 items-center justify-center rounded-full bg-surface-control text-secondary">
          <ChevronsUpDown aria-hidden className="size-2.5" strokeWidth={2.5} />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content
          position="item-aligned"
          className="z-50 min-w-(--radix-select-trigger-width) overflow-hidden rounded-[10px] p-1 material-glass shadow-raised data-[state=open]:animate-fade-in"
        >
          <SelectPrimitive.Viewport>
            {options.map((option) => (
              <SelectPrimitive.Item
                key={option.value}
                value={option.value}
                className="relative flex h-[22px] items-center rounded-[5px] pr-3 pl-6 text-body text-primary outline-none data-highlighted:bg-accent data-highlighted:text-on-accent data-disabled:opacity-40"
              >
                <SelectPrimitive.ItemIndicator className="absolute left-1.5 inline-flex">
                  <Check aria-hidden className="size-3" strokeWidth={2.5} />
                </SelectPrimitive.ItemIndicator>
                <SelectPrimitive.ItemText>{option.label}</SelectPrimitive.ItemText>
              </SelectPrimitive.Item>
            ))}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
