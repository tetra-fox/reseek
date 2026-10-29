import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

// tailwind-merge only knows tailwind's default scales, so without these a class like h-11 can't
// override h-control
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      spacing: ["control", "control-sm", "section", "group", "inline", "gutter"],
      radius: ["control", "card", "badge"],
      shadow: ["card", "raised"],
      container: ["page"],
      animate: ["popover-in", "popover-out", "fade-in", "fade-out", "sheet-in", "sheet-out"],
      ease: ["out-soft"]
    }
  }
});

/** Class names joined, a later Tailwind class winning over an earlier one it conflicts with */
export const cn = (...inputs: ClassValue[]) => twMerge(clsx(inputs));
