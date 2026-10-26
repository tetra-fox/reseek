<script lang="ts">
  import type { HTMLButtonAttributes } from "svelte/elements";

  import { cn } from "#lib/utils.js";

  type Props = HTMLButtonAttributes & {
    variant?: "primary" | "secondary" | "outline" | "ghost" | "danger" | "link";
    size?: "default" | "sm" | "icon" | "icon-sm";
    /** renders a link that looks like the button, for navigation */
    href?: string;
  };

  let {
    variant = "primary",
    size = "default",
    type = "button",
    href,
    class: className,
    children,
    ...rest
  }: Props = $props();

  const variants = {
    primary:
      "bg-accent text-accent-fg shadow-xs not-disabled:hover:bg-accent-hover not-disabled:active:bg-accent-press",
    secondary: "bg-surface-muted text-fg not-disabled:hover:bg-hover not-disabled:active:bg-press",
    outline:
      "border border-input bg-surface text-fg shadow-xs not-disabled:hover:bg-hover not-disabled:active:bg-press",
    ghost: "text-fg not-disabled:hover:bg-hover not-disabled:active:bg-press",
    danger:
      "bg-danger text-danger-fg not-disabled:hover:bg-danger-hover not-disabled:active:bg-danger-press",
    link: "text-accent underline-offset-2 not-disabled:hover:underline"
  };

  const sizes = {
    "default": "h-control px-3 text-sm has-[>svg]:px-2.5",
    "sm": "h-control-sm px-2.5 text-sm has-[>svg]:px-2",
    "icon": "size-control",
    "icon-sm": "size-control-sm"
  };
</script>

<!-- link buttons sit inside a line of text, so they take its size instead of a control height.
     not-disabled rather than enabled, since a link never matches :enabled -->
<svelte:element
  this={href ? "a" : "button"}
  {href}
  type={href ? undefined : type}
  class={cn(
    "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-control font-medium whitespace-nowrap transition duration-(--motion-fast) select-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
    variants[variant],
    variant !== "link" && sizes[size],
    className
  )}
  {...rest}>
  {@render children?.()}
</svelte:element>
