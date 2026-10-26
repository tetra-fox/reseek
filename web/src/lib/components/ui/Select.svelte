<script lang="ts" generics="T">
  import CheckIcon from "@lucide/svelte/icons/check";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import { Select } from "bits-ui";

  import { cn } from "#lib/utils.js";

  type Props = {
    "value": T;
    "options": { value: T; label: string }[];
    "id"?: string;
    "aria-label"?: string;
    "disabled"?: boolean;
    "class"?: string;
  };

  let {
    value = $bindable(),
    options,
    id,
    "aria-label": ariaLabel,
    disabled,
    "class": className
  }: Props = $props();

  // bits-ui values are strings, so items are keyed by index and callers keep their own value type
  const items = $derived(options.map((o, i) => ({ value: String(i), label: o.label })));
</script>

<Select.Root
  type="single"
  {items}
  {disabled}
  value={String(options.findIndex((o) => o.value === value))}
  onValueChange={(i) => (value = options[Number(i)].value)}>
  <Select.Trigger
    {id}
    aria-label={ariaLabel}
    class={cn(
      "flex h-control w-full items-center justify-between gap-2 rounded-control border border-input bg-surface pr-2 pl-3 text-sm whitespace-nowrap shadow-xs transition-colors duration-(--motion-fast) select-none hover:border-fg-subtle/50 disabled:opacity-50 data-[state=open]:border-accent",
      className
    )}>
    <Select.Value class="truncate" />
    <ChevronDownIcon class="size-4 shrink-0 text-fg-muted" />
  </Select.Trigger>
  <Select.Portal>
    <Select.Content
      sideOffset={4}
      class="z-50 max-h-(--bits-select-content-available-height) min-w-(--bits-select-anchor-width) origin-(--bits-select-content-transform-origin) overflow-y-auto rounded-control border bg-overlay p-1 text-fg shadow-raised data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
      {#each items as item (item.value)}
        <Select.Item
          value={item.value}
          label={item.label}
          class="flex h-control-sm items-center justify-between gap-4 rounded-badge px-2 text-sm select-none data-highlighted:bg-hover">
          {#snippet children({ selected })}
            {item.label}
            {#if selected}<CheckIcon class="size-4 text-fg-muted" />{/if}
          {/snippet}
        </Select.Item>
      {/each}
    </Select.Content>
  </Select.Portal>
</Select.Root>
