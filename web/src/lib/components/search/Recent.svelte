<script lang="ts">
  import HistoryIcon from "@lucide/svelte/icons/history";
  import PinIcon from "@lucide/svelte/icons/pin";
  import PinOffIcon from "@lucide/svelte/icons/pin-off";
  import XIcon from "@lucide/svelte/icons/x";

  import { toast } from "#lib/feedback/toast.svelte.js";
  import { forget, recent, restore, togglePin } from "#lib/search/recent.svelte.js";

  type Props = {
    /** runs a search again, while the server is connected */
    onsearch: (text: string) => void;
    connected: boolean;
  };

  let { onsearch, connected }: Props = $props();

  const remove = (search: string) => {
    const at = forget(search);
    toast(`Removed '${search}' from recent searches`, "info", {
      action: { label: "Undo", run: () => restore(search, at) }
    });
  };
</script>

{#if recent.pinned.length || recent.texts.length}
  <ul class="flex flex-wrap items-center gap-1.5" aria-label="Pinned and recent searches">
    {#each recent.pinned as search (`pin:${search}`)}
      <li class="group flex items-center rounded-full border bg-accent-soft text-accent-soft-fg">
        <button
          type="button"
          class="flex items-center gap-1 py-0.5 pl-2.5 text-xs disabled:opacity-50"
          title={`Search for ${search} again`}
          disabled={!connected}
          onclick={() => onsearch(search)}>
          <PinIcon class="size-3" />{search}
        </button>
        <button
          type="button"
          class="rounded-full p-1 opacity-60 hover:opacity-100"
          aria-label={`Unpin ${search}`}
          onclick={() => togglePin(search)}>
          <PinOffIcon class="size-3" />
        </button>
      </li>
    {/each}
    {#each recent.texts as search (`recent:${search}`)}
      <li class="group flex items-center rounded-full border text-fg-muted">
        <button
          type="button"
          class="flex items-center gap-1 py-0.5 pl-2.5 text-xs hover:text-fg disabled:opacity-50"
          title={`Search for ${search} again`}
          disabled={!connected}
          onclick={() => onsearch(search)}>
          <HistoryIcon class="size-3" />{search}
        </button>
        <button
          type="button"
          class="rounded-full p-1 row-action"
          aria-label={`Pin ${search}`}
          onclick={() => togglePin(search)}>
          <PinIcon class="size-3" />
        </button>
        <button
          type="button"
          class="rounded-full p-1 pr-1.5 row-action"
          aria-label={`Remove ${search} from recent searches`}
          onclick={() => remove(search)}>
          <XIcon class="size-3" />
        </button>
      </li>
    {/each}
  </ul>
{/if}
