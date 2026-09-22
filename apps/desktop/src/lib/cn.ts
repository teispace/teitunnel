import { type ClassValue, clsx } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

/**
 * tailwind-merge taught about our design tokens, so custom utilities aren't mistaken for
 * conflicts (e.g. `text-body` is a size and `text-primary` a colour; `border-hairline`
 * is a width and `border-control` a colour).
 */
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      text: [
        "large-title",
        "title1",
        "title2",
        "title3",
        "headline",
        "body",
        "callout",
        "footnote",
        "mono",
      ],
      radius: ["control", "row", "card", "sheet"],
      shadow: ["control", "raised", "sheet"],
    },
    classGroups: {
      "border-w": ["border-hairline"],
      "border-w-t": ["border-t-hairline"],
      "border-w-b": ["border-b-hairline"],
    },
  },
});

/** Joins class names and resolves Tailwind conflicts (last one wins). */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
