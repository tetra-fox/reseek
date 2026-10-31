<script lang="ts">
  import { goto } from "$app/navigation";
  import HistoryIcon from "@lucide/svelte/icons/history";
  import LinkIcon from "@lucide/svelte/icons/link";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";

  import { app } from "#lib/app.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Marked from "#lib/components/ui/Marked.svelte";
  import * as Suggest from "#lib/components/ui/suggest/suggest.js";
  import UserEntries from "#lib/components/users/UserEntries.svelte";
  import { parseSlsk, slskBrowse } from "#lib/files/slsk.js";
  import { recent } from "#lib/search/recent.svelte.js";
  import { openSearch, type SearchScope, scopeLabel } from "#lib/search/searches.svelte.js";
  import { UserFinder } from "#lib/users/finder.svelte.js";
  import { bestMatches, nameParts } from "#lib/users/match.js";

  import Recent from "./Recent.svelte";

  type Props = {
    /** where the search goes; the whole network when not given */
    scope?: SearchScope;
    /** drops the scope, back to the whole network */
    onclearscope?: () => void;
    /** the text the box starts with */
    text?: string;
    /** lists the recent and pinned searches under the box */
    history?: boolean;
  };

  let { scope, onclearscope, text: initial = "", history = false }: Props = $props();

  /** how many recent searches the dropdown lists */
  const EARLIER = 4;
  /** the entry that runs what is in the box, the first of the dropdown */
  const RUN = "run";
  const EARLIER_PREFIX = "earlier:";

  // the box starts from what the page was opened with and is the user's from then on
  // svelte-ignore state_referenced_locally
  let text = $state(initial);
  let open = $state(false);

  const phrase = $derived(text.trim());
  // a pasted slsk:// link opens what it points at rather than searching for its text
  const link = $derived(parseSlsk(phrase));
  const connected = $derived(app.isServerConnected);
  const scoped = $derived(scope ? scopeLabel(scope) : null);
  const earlier = $derived(
    bestMatches([...recent.pinned, ...recent.texts], phrase, (t) => t)
      .filter((t) => t !== phrase)
      .slice(0, EARLIER)
  );
  // a search of one user's shares and a link have no user to offer
  const finder = new UserFinder(() => (scope || link ? "" : phrase), {
    tab: "shares",
    onmissed: () => (open = true)
  });

  const search = (words: string) => openSearch(words, scope);

  const run = () => {
    if (link) {
      text = "";
      void goto(slskBrowse(link));
    } else if (phrase && connected) search(phrase);
  };

  const choose = (value: string) => {
    if (value === RUN) run();
    else if (value.startsWith(EARLIER_PREFIX)) search(value.slice(EARLIER_PREFIX.length));
    else finder.choose(value);
  };
</script>

<form
  class="flex flex-col gap-2 rounded-card border bg-surface p-3 shadow-card"
  onsubmit={(e) => {
    e.preventDefault();
    run();
  }}>
  <!-- the button isn't disabled while the box is empty: a browser ignores Enter when the form's
       default button is disabled, and run already ignores an empty phrase -->
  <div class="flex items-center gap-inline">
    <!-- Enter takes the first entry, so it searches unless the keys moved to another -->
    <Suggest.Box
      bind:value={text}
      bind:open
      first={RUN}
      hold={finder.asking}
      onchoose={choose}
      ondismiss={finder.abandon}
      type="search"
      enterkeyhint="search"
      autocomplete="off"
      icon={SearchIcon}
      wrapperClass="flex-1"
      autofocus
      data-lpignore="true"
      aria-label="Search for files"
      placeholder="Search for files"
      disabled={!connected}
      class="h-11 pl-10 text-base md:text-base">
      <Suggest.Item value={RUN} icon={link ? LinkIcon : SearchIcon}>
        {link ? "Open this slsk:// link" : `Search for ${phrase}`}
      </Suggest.Item>
      {#if earlier.length}
        <Suggest.Group heading="Recent searches">
          {#each earlier as words (words)}
            <Suggest.Item value={`${EARLIER_PREFIX}${words}`} icon={HistoryIcon}>
              <Marked parts={nameParts(words, phrase)} />
            </Suggest.Item>
          {/each}
        </Suggest.Group>
      {/if}
      {#if finder.matches.length || finder.unknown}
        <Suggest.Group heading="Users">
          <UserEntries {finder} text={phrase} />
        </Suggest.Group>
      {/if}
    </Suggest.Box>
    <Button type="submit" class="h-11 px-4" disabled={!connected}>Search</Button>
  </div>
  {#if app.state && !connected}
    <p class="text-xs text-fg-muted">Connect to the Soulseek server to search.</p>
  {/if}
  {#if scoped}
    <div class="flex items-center gap-2 text-xs text-fg-muted">
      Search limited to
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
  {#if history}
    <Recent onsearch={search} {connected} />
  {/if}
</form>
