<script lang="ts">
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import HistoryIcon from "@lucide/svelte/icons/history";
  import PinIcon from "@lucide/svelte/icons/pin";
  import PinOffIcon from "@lucide/svelte/icons/pin-off";
  import XIcon from "@lucide/svelte/icons/x";

  import { browsed, forgetBrowsed, restoreBrowsed } from "#lib/browse/recent.svelte.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { undoable } from "#lib/feedback/undo.js";
  import { sharesHref } from "#lib/nav.js";
  import { forget, recent, restore, togglePin } from "#lib/search/recent.svelte.js";

  // the searches and the users browsed lately, each with a way to remove it
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

  const removeBrowsed = (username: string) => {
    const at = forgetBrowsed(username);
    undoable(
      `Removed ${username} from recently browsed`,
      () => {},
      () => restoreBrowsed(username, at)
    );
  };
</script>

{#if recent.pinned.length || recent.texts.length}
  <ul class="flex flex-wrap items-center gap-1.5" aria-label="Pinned and recent searches">
    {#each recent.pinned as search (`pin:${search}`)}
      <li class="group flex items-center rounded-full border bg-accent-soft text-accent-soft-fg">
        <Tooltip text={`Search for ${search} again`}>
          {#snippet child({ props })}
            <button
              {...props}
              type="button"
              class="flex items-center gap-1 py-0.5 pl-2.5 text-xs disabled:opacity-50"
              disabled={!connected}
              onclick={() => onsearch(search)}>
              <PinIcon class="size-3" />{search}
            </button>
          {/snippet}
        </Tooltip>
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
        <Tooltip text={`Search for ${search} again`}>
          {#snippet child({ props })}
            <button
              {...props}
              type="button"
              class="flex items-center gap-1 py-0.5 pl-2.5 text-xs hover:text-fg disabled:opacity-50"
              disabled={!connected}
              onclick={() => onsearch(search)}>
              <HistoryIcon class="size-3" />{search}
            </button>
          {/snippet}
        </Tooltip>
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
{#if browsed.users.length}
  <ul class="flex flex-wrap items-center gap-1.5" aria-label="Recently browsed">
    {#each browsed.users as name (name)}
      <li class="group flex items-center rounded-full border text-fg-muted">
        <Tooltip text={`Browse ${name}'s shares`}>
          {#snippet child({ props })}
            <a
              {...props}
              href={sharesHref(name)}
              class="flex items-center gap-1 py-0.5 pl-2.5 text-xs hover:text-fg">
              <FolderOpenIcon class="size-3" />{name}
            </a>
          {/snippet}
        </Tooltip>
        <button
          type="button"
          class="rounded-full p-1 pr-1.5 row-action"
          aria-label={`Remove ${name} from recently browsed`}
          onclick={() => removeBrowsed(name)}>
          <XIcon class="size-3" />
        </button>
      </li>
    {/each}
  </ul>
{/if}
