<script lang="ts" generics="T extends string">
  import { RadioGroup } from "bits-ui";
  import type { Component } from "svelte";

  import { cn } from "#lib/ui/cn.js";

  // a choice of one among a few, all in view, on more lines where a large text size leaves no room
  // on one. The `sm` size stands beside `sm` buttons, at their height, and the chosen one keeps the
  // weight of the others, so none of them moves
  type Props = {
    "options": readonly { id: T; label: string; icon?: Component }[];
    "value": T;
    /** what is being chosen, for a screen reader */
    "label": string;
    "size"?: "xs" | "sm";
    "id"?: string;
    "aria-describedby"?: string;
    "disabled"?: boolean;
    "class"?: string;
  };

  let {
    options,
    value = $bindable(),
    label,
    size = "xs",
    disabled = false,
    class: className,
    ...rest
  }: Props = $props();
</script>

<!-- the arrows move to the next choice and take it, round the ends, as radio buttons do. The
     values bits-ui gives back are only the options' ids -->
<RadioGroup.Root
  bind:value={() => value, (v) => (value = v as T)}
  orientation="horizontal"
  loop
  {disabled}
  aria-label={label}
  class={cn(
    "inline-flex max-w-full flex-wrap rounded-control border p-0.5",
    size === "sm" ? "min-h-control-sm" : "min-h-6.5",
    className
  )}
  {...rest}>
  {#each options as option (option.id)}
    <RadioGroup.Item
      value={option.id}
      class={[
        "flex items-center gap-1.5 rounded-badge px-2 text-fg-muted transition-colors duration-(--motion-fast) not-disabled:hover:text-fg disabled:opacity-50 data-[state=checked]:bg-surface-muted data-[state=checked]:text-fg",
        size === "xs" ? "text-xs" : "text-sm"
      ]}>
      {#if option.icon}<option.icon class="size-3.5" />{/if}{option.label}
    </RadioGroup.Item>
  {/each}
</RadioGroup.Root>
