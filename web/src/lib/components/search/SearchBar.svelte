<script lang="ts">
  import { goto } from "$app/navigation";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";

  import { app } from "#lib/app.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { href } from "#lib/nav.js";
  import { type SearchScope, scopeLabel, searches } from "#lib/search/searches.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  type Props = {
    /** where the search goes; the whole network when not given */
    scope?: SearchScope;
    /** drops the scope, back to the whole network */
    onclearscope?: () => void;
    /** the text the box starts with */
    text?: string;
    /** says in a toast that the search started, where the new search doesn't show in a list */
    announce?: boolean;
  };

  let { scope, onclearscope, text: initial = "", announce = false }: Props = $props();

  // the box starts from what the page was opened with and is the user's from then on
  // svelte-ignore state_referenced_locally
  let text = $state(initial);
  let creating = $state(false);
  let input = $state<HTMLInputElement | null>(null);

  const connected = $derived(app.isServerConnected);
  const disabled = $derived(creating || !connected);
  const scoped = $derived(scope ? scopeLabel(scope) : null);

  /** Starts the search, then opens it or stays; a second submit while one runs is ignored */
  const submit = async (open: boolean) => {
    const phrase = text.trim();
    if (!phrase || creating) return;
    creating = true;
    const outcome = await searches.create(phrase, scope);
    creating = false;
    if (!outcome.ok) {
      toast(outcome.error, "error");
      return;
    }
    text = "";
    const to = href(`/searches/${outcome.data.id}`);
    if (open) {
      void goto(to);
      return;
    }
    if (announce) {
      toast(`Search for '${phrase}' started`, "info", {
        action: { label: "View results", run: () => void goto(to) }
      });
    }
    // the box gets focus back once it is enabled again
    queueMicrotask(() => input?.focus());
  };
</script>

<form
  class="flex flex-col gap-2 rounded-card border bg-surface p-3 shadow-card"
  onsubmit={(e) => {
    e.preventDefault();
    void submit(false);
  }}>
  <div class="flex items-center gap-inline">
    <div class="relative min-w-0 flex-1">
      <SearchIcon
        class="pointer-events-none absolute top-1/2 left-3 size-5 -translate-y-1/2 text-fg-muted" />
      <!-- svelte-ignore a11y_autofocus -->
      <input
        bind:this={input}
        bind:value={text}
        type="search"
        autofocus
        data-lpignore="true"
        aria-label="Search phrase"
        placeholder={connected ? "Search phrase" : "Connect to server to perform a search"}
        {disabled}
        class="h-11 w-full min-w-0 rounded-control border border-input bg-surface pr-10 pl-10 text-base text-fg shadow-xs placeholder:text-fg-muted focus-visible:border-accent disabled:opacity-50" />
      {#if creating}
        <Spinner class="absolute top-1/2 right-3 size-4 -translate-y-1/2" label="Starting" />
      {/if}
    </div>
    <Button
      type="submit"
      variant="outline"
      size="icon"
      class="size-11"
      aria-label="Search and stay here"
      disabled={disabled || !text.trim()}>
      <PlusIcon />
    </Button>
    <Button
      size="icon"
      class="size-11"
      aria-label="Search and show the results"
      disabled={disabled || !text.trim()}
      onclick={() => void submit(true)}>
      <SearchIcon />
    </Button>
  </div>
  {#if scoped}
    <div class="flex items-center gap-2 text-xs text-fg-muted">
      Only searching
      <Badge tone="accent">
        {scoped}
        {#if onclearscope}
          <button type="button" aria-label="Search the whole network" onclick={onclearscope}>
            <XIcon />
          </button>
        {/if}
      </Badge>
    </div>
  {/if}
</form>
