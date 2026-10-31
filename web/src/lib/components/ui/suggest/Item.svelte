<script lang="ts">
  import { Combobox } from "bits-ui";
  import type { Component, Snippet } from "svelte";

  import { cn } from "#lib/ui/cn.js";

  import { highlightOf } from "./highlight";

  type Props = {
    value: string;
    disabled?: boolean;
    /** an icon before the entry, which then holds one line of text */
    icon?: Component;
    class?: string;
    children: Snippet;
  };

  let { value, disabled, icon: Icon, class: className, children }: Props = $props();

  const highlight = highlightOf();
</script>

<!-- the highlight is what Enter takes, so only the keys move it; the pointer gets a tint of its
     own, and stopping its move here keeps bits-ui from moving the highlight along with it. The two
     can sit on neighboring entries, so the tints stop short of the entry's edges -->
<Combobox.Item
  {value}
  {disabled}
  onHighlight={() => (highlight.lit = value)}
  onUnhighlight={() => {
    if (highlight.lit === value) highlight.lit = null;
  }}
  onpointermove={(e) => e.preventDefault()}
  class={cn(
    "flex min-h-control-sm items-center gap-2 rounded-badge border-y border-transparent bg-clip-padding px-2 py-0.5 text-sm not-data-disabled:hover:bg-hover/60 data-highlighted:bg-hover data-highlighted:not-data-disabled:hover:bg-hover",
    className
  )}>
  {#if Icon}
    <Icon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
    <span class="min-w-0 truncate">{@render children()}</span>
  {:else}
    {@render children()}
  {/if}
</Combobox.Item>
