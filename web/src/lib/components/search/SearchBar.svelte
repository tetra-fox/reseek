<script lang="ts">
  import { goto } from "$app/navigation";
  import HistoryIcon from "@lucide/svelte/icons/history";
  import PinIcon from "@lucide/svelte/icons/pin";
  import PinOffIcon from "@lucide/svelte/icons/pin-off";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";

  import { app } from "#lib/app.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { href } from "#lib/nav.js";
  import { forget, recent, remember, restore, togglePin } from "#lib/search/recent.svelte.js";
  import { type SearchScope, scopeLabel, searches } from "#lib/search/searches.svelte.js";
  import { parseSlsk, slskBrowse } from "#lib/slsk.js";
  import { toast } from "#lib/toast.svelte.js";

  type Props = {
    /** where the search goes; the whole network when not given */
    scope?: SearchScope;
    /** drops the scope, back to the whole network */
    onclearscope?: () => void;
    /** the text the box starts with */
    text?: string;
    /** says in a toast that a search started from + where it doesn't show in a list */
    announce?: boolean;
    /** lists the recent and pinned searches under the box */
    history?: boolean;
  };

  let {
    scope,
    onclearscope,
    text: initial = "",
    announce = false,
    history = false
  }: Props = $props();

  // the box starts from what the page was opened with and is the user's from then on
  // svelte-ignore state_referenced_locally
  let text = $state(initial);
  let creating = $state(false);
  let input = $state<HTMLInputElement | null>(null);

  const connected = $derived(app.isServerConnected);
  const disabled = $derived(creating || !connected);
  const scoped = $derived(scope ? scopeLabel(scope) : null);

  /** Opens a search's page at once; the results stream in as the daemon finds them */
  const open = (phrase: string) => {
    const { id, done } = searches.start(phrase, scope);
    remember(phrase);
    void goto(href(`/searches/${id}`));
    // a search refused while its page is open says so there; elsewhere it toasts
    void done.then((outcome) => {
      if (!outcome.ok && !location.hash.includes(id)) toast(outcome.error, "error");
    });
  };

  /** Starts the search and stays; a second submit while one runs is ignored */
  const stay = async (phrase: string) => {
    creating = true;
    const outcome = await searches.create(phrase, scope);
    creating = false;
    if (!outcome.ok) {
      toast(outcome.error, "error");
      return;
    }
    remember(phrase);
    text = "";
    if (announce) {
      const to = href(`/searches/${outcome.data.id}`);
      const label = phrase.length > 30 ? `${phrase.slice(0, 15)}...` : phrase;
      toast(`Search for '${label}' started`, "info", {
        action: { label: "View results", run: () => void goto(to) }
      });
    }
    // the box gets focus back once it is enabled again
    queueMicrotask(() => input?.focus());
  };

  const submit = (opening: boolean) => {
    const phrase = text.trim();
    // a pasted slsk:// link opens what it points at rather than searching for its text
    const link = parseSlsk(phrase);
    if (link) {
      text = "";
      void goto(slskBrowse(link));
      return;
    }
    if (!phrase || creating || !connected) return;
    if (opening) open(phrase);
    else void stay(phrase);
  };

  const remove = (search: string) => {
    const at = forget(search);
    toast(`Removed '${search}' from recent searches`, "info", {
      action: { label: "Undo", run: () => restore(search, at) }
    });
  };
</script>

<form
  class="flex flex-col gap-2 rounded-card border bg-surface p-3 shadow-card"
  onsubmit={(e) => {
    e.preventDefault();
    submit(true);
  }}>
  <div class="flex items-center gap-inline">
    <div class="relative min-w-0 flex-1">
      <Input
        bind:ref={input}
        bind:value={text}
        type="search"
        icon={SearchIcon}
        autofocus
        data-lpignore="true"
        aria-label="Search phrase"
        placeholder={connected ? "Search phrase" : "Connect to server to perform a search"}
        {disabled}
        class="h-11 pl-10 text-base md:text-base" />
      {#if creating}
        <Spinner class="absolute top-1/2 right-9 size-4 -translate-y-1/2" label="Starting" />
      {/if}
    </div>
    <Button
      variant="outline"
      size="icon"
      class="size-11"
      aria-label="Search and stay here"
      title="Search and stay here"
      disabled={disabled || !text.trim()}
      onclick={() => submit(false)}>
      <PlusIcon />
    </Button>
    <Button
      type="submit"
      size="icon"
      class="size-11"
      aria-label="Search and show the results"
      title="Search and show the results (Enter)"
      disabled={disabled || !text.trim()}>
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
  {#if history && (recent.pinned.length || recent.texts.length)}
    <ul class="flex flex-wrap items-center gap-1.5" aria-label="Pinned and recent searches">
      {#each recent.pinned as search (`pin:${search}`)}
        <li class="group flex items-center rounded-full border bg-accent-soft text-accent-soft-fg">
          <button
            type="button"
            class="flex items-center gap-1 py-0.5 pl-2.5 text-xs disabled:opacity-50"
            title={`Search for ${search} again`}
            disabled={!connected}
            onclick={() => open(search)}>
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
            onclick={() => open(search)}>
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
</form>
