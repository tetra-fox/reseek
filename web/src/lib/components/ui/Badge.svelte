<script lang="ts">
  import type { HTMLAttributes } from "svelte/elements";

  import { cn } from "#lib/ui/cn.js";

  type Props = HTMLAttributes<HTMLSpanElement> & {
    tone?: "neutral" | "accent" | "danger" | "warning" | "success" | "info" | "outline";
  };

  let { tone = "neutral", class: className, children, ...rest }: Props = $props();

  const tones = {
    // the border keeps it visible on a zebra row, which shares its fill
    neutral: "border-border bg-surface-muted text-fg-muted",
    accent: "border-transparent bg-accent-soft text-accent-soft-fg",
    danger: "border-transparent bg-danger-soft text-danger-soft-fg",
    warning: "border-transparent bg-warning-soft text-warning-soft-fg",
    success: "border-transparent bg-success-soft text-success-soft-fg",
    info: "border-transparent bg-info-soft text-info-soft-fg",
    outline: "border-input text-fg-muted"
  };
</script>

<!-- a block of one line rather than a flex box, whose text would be an anonymous item that
     cannot be cut with an ellipsis: a badge never grows past its container -->
<span
  class={cn(
    "inline-block max-w-full shrink-0 truncate rounded-badge border px-1.5 py-0.5 align-middle text-xs font-medium [&>svg]:mr-1 [&>svg]:inline [&>svg]:size-3 [&>svg]:align-[-0.125em]",
    tones[tone],
    className
  )}
  data-label
  {...rest}>
  {@render children?.()}
</span>
