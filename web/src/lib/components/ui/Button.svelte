<script lang="ts">
  import type { HTMLButtonAttributes } from "svelte/elements";

  import { cn } from "#lib/ui/cn.js";

  type Props = HTMLButtonAttributes & {
    variant?: "primary" | "secondary" | "outline" | "ghost" | "danger" | "link";
    size?: "default" | "sm" | "icon" | "icon-sm";
    /** renders a link that looks like the button, for navigation */
    href?: string;
    /** with `href`, saves what the link points at under this name */
    download?: string;
    /** with `href`, where the link opens and how it relates, as on a link */
    target?: string;
    rel?: string;
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
      "border-transparent bg-accent text-accent-fg shadow-xs not-disabled:hover:bg-accent-hover not-disabled:active:bg-accent-press",
    secondary:
      "border-transparent bg-surface-muted text-fg not-disabled:hover:bg-hover not-disabled:active:bg-press",
    outline:
      "border-input bg-surface text-fg shadow-xs not-disabled:hover:bg-hover not-disabled:active:bg-press",
    ghost: "border-transparent text-fg not-disabled:hover:bg-hover not-disabled:active:bg-press",
    danger:
      "border-transparent bg-danger text-danger-fg not-disabled:hover:bg-danger-hover not-disabled:active:bg-danger-press",
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
     not-disabled rather than enabled, since a link never matches :enabled. Every other variant has
     a border, clear where it doesn't show, so a toggle between two keeps its width -->
<svelte:element
  this={href ? "a" : "button"}
  {href}
  type={href ? undefined : type}
  class={cn(
    "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-control font-medium whitespace-nowrap transition-[color,background-color,border-color,opacity,scale] duration-(--motion-fast) ease-out-soft not-disabled:active:scale-[0.97] disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
    variants[variant],
    variant !== "link" && ["border", sizes[size]],
    className
  )}
  data-label
  {...rest}>
  {@render children?.()}
</svelte:element>
