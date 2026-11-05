<script lang="ts">
  import SlidersHorizontalIcon from "@lucide/svelte/icons/sliders-horizontal";
  import type { Snippet } from "svelte";

  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import { type ActiveFilter, clearAll } from "#lib/lists/filtered.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  import { FILTERING } from "./tint";

  // a screen's filter controls: in line on a wide screen, and on a phone, which has no room for
  // them beside the list, behind a Filters button that says how many are on and opens them
  type Props = {
    /** the filters the controls inside set, which the button counts and Clear filters clears */
    filters: ActiveFilter[];
    children: Snippet;
  };

  let { filters, children }: Props = $props();

  let open = $state(false);
</script>

{#if viewport.phone}
  <Button
    variant="outline"
    size="sm"
    class={[filters.length && FILTERING]}
    aria-haspopup="dialog"
    onclick={() => (open = true)}>
    <SlidersHorizontalIcon />Filters{#if filters.length}
      <span class="tabular-nums">({filters.length})</span>{/if}
  </Button>
  <Dialog bind:open title="Filters">
    {@render children()}
    {#snippet footer()}
      <Button variant="outline" disabled={!filters.length} onclick={() => clearAll(filters)}
        >Clear filters</Button>
      <Button onclick={() => (open = false)}>Done</Button>
    {/snippet}
  </Dialog>
{:else}
  {@render children()}
{/if}
