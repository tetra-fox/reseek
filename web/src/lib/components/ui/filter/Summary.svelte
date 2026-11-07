<script lang="ts">
  import FunnelIcon from "@lucide/svelte/icons/funnel";
  import XIcon from "@lucide/svelte/icons/x";
  import { tick, untrack } from "svelte";

  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import {
    type ActiveFilter,
    announce,
    clearThen,
    focusAfter,
    type Noun,
    showing
  } from "#lib/lists/filtered.js";
  import { cn } from "#lib/ui/cn.js";

  // the line above a filtered list: how much of it shows, a chip for each filter that takes that
  // one off, and Clear filters. Nothing while no filter is on, or while the list holds nothing
  type Props = {
    filters: ActiveFilter[];
    shown: number;
    /** everything the list holds; null for a list that counts only what matched */
    total: number | null;
    noun: Noun;
    /** gives focus to the list once no chip is left to take it */
    refocus: () => void;
    /** turns every filter off at once, where turning each off in turn does not */
    onclear?: () => void;
    class?: string;
  };

  let { filters, shown, total, noun, refocus, onclear, class: className }: Props = $props();

  let chips = $state<HTMLElement | null>(null);
  /** what the status line says, set as the filters change, so opening a screen says nothing */
  let said = $state("");
  /**
   * the chip to focus once a removal has taken its chip away, null for the list; the removal can
   * land later than the click, as a filter kept in the URL does
   */
  let after: { id: string | null } | null = null;
  // which filters are on, not what they say, so typing in a text box says nothing until it empties
  const ids = $derived(filters.map((f) => f.id).join("\n"));
  let last = untrack(() => ids);
  $effect(() => {
    if (ids === last) return;
    last = ids;
    said = announce(untrack(() => filters));
    if (!after) return;
    const { id } = after;
    after = null;
    const chip =
      id === null ? null : chips?.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"]`);
    // the list may come back in this same update, so it is focused once that is done
    if (chip) chip.focus();
    else void tick().then(refocus);
  });

  const remove = (at: number) => {
    const next = focusAfter(at, filters.length);
    after = { id: next === null ? null : filters[next].id };
    filters[at].clear();
  };
</script>

<p class="sr-only" role="status">{said}</p>
{#if filters.length && total !== 0}
  <div
    class={cn(
      "flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-control border border-accent/40 bg-accent-soft px-3 py-1.5 text-sm text-accent-soft-fg",
      className
    )}>
    <span class="flex items-center gap-1.5 font-medium tabular-nums">
      <FunnelIcon class="size-4 shrink-0" />{showing(shown, total, noun)}
    </span>
    <div class="flex flex-wrap items-center gap-1.5" bind:this={chips}>
      {#each filters as f, at (f.id)}
        <Tooltip text={`Remove filter ${f.label}`}>
          {#snippet child({ props })}
            <button
              {...props}
              type="button"
              class="inline-flex h-6 max-w-64 items-center gap-1 rounded-full border border-accent/40 bg-surface pr-1.5 pl-2.5 text-xs text-fg transition-colors duration-(--motion-fast) hover:border-accent"
              aria-label={`Remove filter ${f.label}`}
              data-id={f.id}
              onclick={() => remove(at)}>
              <span class="truncate">{f.label}</span><XIcon class="size-3 shrink-0" />
            </button>
          {/snippet}
        </Tooltip>
      {/each}
    </div>
    <!-- an emptied list offers its own Clear filters where the rows would be -->
    {#if shown > 0}
      <button
        type="button"
        class="ml-auto font-medium underline underline-offset-2 hover:no-underline"
        onclick={() => void clearThen(filters, onclear, refocus)}>Clear filters</button>
    {/if}
  </div>
{/if}
