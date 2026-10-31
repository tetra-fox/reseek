<script lang="ts" generics="T extends string">
  import type { Component } from "svelte";

  import { cn } from "#lib/ui/cn.js";

  // a choice of one among a few, all in view. The `sm` size stands beside `sm` buttons, at their
  // height, and the chosen one keeps the weight of the others, so none of them moves
  type Props = {
    options: readonly { id: T; label: string; icon?: Component }[];
    value: T;
    /** what is being chosen, for a screen reader */
    label: string;
    size?: "xs" | "sm";
    class?: string;
  };

  let { options, value = $bindable(), label, size = "xs", class: className }: Props = $props();

  /** The arrows move to the next choice and take it, round the ends, as radio buttons do */
  const arrow = (e: KeyboardEvent & { currentTarget: HTMLElement }, at: number) => {
    const forward = e.key === "ArrowRight" || e.key === "ArrowDown";
    if (!forward && e.key !== "ArrowLeft" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const next = (at + (forward ? 1 : -1) + options.length) % options.length;
    value = options[next].id;
    (e.currentTarget.parentElement?.children[next] as HTMLElement | undefined)?.focus();
  };
</script>

<div
  class={cn(
    "inline-flex rounded-control border p-0.5",
    size === "sm" ? "h-control-sm" : "h-6.5",
    className
  )}
  role="radiogroup"
  aria-label={label}>
  {#each options as option, at (option.id)}
    <button
      type="button"
      role="radio"
      aria-checked={value === option.id}
      tabindex={value === option.id ? 0 : -1}
      class={[
        "flex items-center gap-1.5 rounded-badge px-2 transition-colors duration-(--motion-fast)",
        size === "xs" ? "text-xs" : "text-sm",
        value === option.id ? "bg-surface-muted text-fg" : "text-fg-muted hover:text-fg"
      ]}
      onclick={() => (value = option.id)}
      onkeydown={(e) => arrow(e, at)}>
      {#if option.icon}<option.icon class="size-3.5" />{/if}{option.label}
    </button>
  {/each}
</div>
