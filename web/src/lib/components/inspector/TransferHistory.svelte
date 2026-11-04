<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import { formatDate, formatDuration } from "#lib/format.js";
  import { SETTLE_MS } from "#lib/inspector/inspector.svelte.js";
  import { storeOf } from "#lib/transfers/actions.js";
  import { stateChip, type Transfer } from "#lib/transfers/states.js";

  // the states a transfer went through, each with when and how long after the one before
  let { transfer: t }: { transfer: Transfer } = $props();

  let history = $state.raw<Schemas["TransferStateChange"][] | null>(null);
  /** why the history could not be read, empty once it has been */
  let error = $state("");

  /** the last history asked for, so an answer to an earlier ask is dropped */
  let asked = 0;
  let historyOf: string | null = null;
  // the history is asked for once focus rests on a transfer and again at each of its new states;
  // the one shown stays until the new one arrives, unless it is another transfer's, whose answer
  // still on its way is dropped
  $effect(() => {
    const shown = t;
    void shown.state;
    if (historyOf !== shown.id) {
      asked++;
      history = null;
      error = "";
    }
    historyOf = shown.id;
    const timer = setTimeout(() => {
      const ask = ++asked;
      void storeOf(shown)
        .history(shown)
        .then((o) => {
          if (ask !== asked) return;
          error = o.ok ? "" : o.error;
          if (o.ok) history = o.data.stateHistory ?? [];
        });
    }, SETTLE_MS);
    return () => clearTimeout(timer);
  });
</script>

{#if history === null && error}
  <p class="text-fg-muted">Could not read the history: {error}</p>
{:else if history === null}
  <p class="text-fg-muted">Loading...</p>
{:else if !history.length}
  <EmptyRow text="No state changes recorded" class="px-0 py-2 text-left" />
{:else}
  <ol class="flex flex-col gap-1">
    {#each history as change, i (`${change.at}:${change.state}`)}
      <li class="flex gap-2">
        <span class="text-fg-muted tabular-nums">{formatDate(change.at)}</span>
        <span class="min-w-0 flex-1">{stateChip({ ...t, state: change.state }).text}</span>
        {#if i > 0}
          <span class="text-xs text-fg-muted">
            +{formatDuration((Date.parse(change.at) - Date.parse(history[i - 1].at)) / 1000)}
          </span>
        {/if}
      </li>
    {/each}
  </ol>
{/if}
