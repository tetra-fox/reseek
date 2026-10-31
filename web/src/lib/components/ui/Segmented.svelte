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
</script>

<div
  class={cn(
    "inline-flex rounded-control border p-0.5",
    size === "sm" ? "h-control-sm" : "h-6.5",
    className
  )}
  role="radiogroup"
  aria-label={label}>
  {#each options as option (option.id)}
    <button
      type="button"
      role="radio"
      aria-checked={value === option.id}
      class={[
        "flex items-center gap-1.5 rounded-badge px-2 transition-colors duration-(--motion-fast)",
        size === "xs" ? "text-xs" : "text-sm",
        value === option.id ? "bg-surface-muted text-fg" : "text-fg-muted hover:text-fg"
      ]}
      onclick={() => (value = option.id)}>
      {#if option.icon}<option.icon class="size-3.5" />{/if}{option.label}
    </button>
  {/each}
</div>
