<script lang="ts">
  import type { HTMLButtonAttributes } from "svelte/elements";

  import { cn } from "#lib/ui/cn.js";

  type Props = Omit<HTMLButtonAttributes, "onclick"> & {
    /** a click, on the button or on the link it renders with `href` */
    onclick?: (e: MouseEvent) => void;
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
    onclick,
    ...rest
  }: Props = $props();

  // aria-disabled keeps the button focusable, so a tooltip can give the reason, and does nothing
  const unusable = $derived(rest["aria-disabled"] === true || rest["aria-disabled"] === "true");

  const variants = {
    primary:
      "border-transparent bg-accent text-accent-fg shadow-xs usable:hover:bg-accent-hover usable:active:bg-accent-press",
    secondary:
      "border-transparent bg-surface-muted text-fg usable:hover:bg-hover usable:active:bg-press",
    outline:
      "border-input bg-surface text-fg shadow-xs usable:hover:bg-hover usable:active:bg-press",
    ghost: "border-transparent text-fg usable:hover:bg-hover usable:active:bg-press",
    danger:
      "border-transparent bg-danger text-danger-fg usable:hover:bg-danger-hover usable:active:bg-danger-press",
    link: "text-accent underline-offset-2 usable:hover:underline"
  };

  const sizes = {
    "default": "h-control px-3 text-sm has-[>svg]:px-2.5",
    "sm": "h-control-sm px-2.5 text-sm has-[>svg]:px-2",
    "icon": "size-control",
    "icon-sm": "size-control-sm"
  };
</script>

<!-- link buttons sit inside a line of text, so they take its size instead of a control height.
     usable rather than enabled, since a link never matches :enabled. Every other variant has
     a border, clear where it doesn't show, so a toggle between two keeps its width -->
<svelte:element
  this={href ? "a" : "button"}
  {href}
  type={href ? undefined : type}
  class={cn(
    "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-control font-medium whitespace-nowrap transition-[color,background-color,border-color,opacity,scale] duration-(--motion-fast) ease-out-soft usable:active:scale-[0.97] unusable:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
    variants[variant],
    variant !== "link" && ["border", sizes[size]],
    className
  )}
  data-label
  {...rest}
  onclick={(e: MouseEvent) => {
    if (unusable) return e.preventDefault();
    onclick?.(e);
  }}>
  {@render children?.()}
</svelte:element>
