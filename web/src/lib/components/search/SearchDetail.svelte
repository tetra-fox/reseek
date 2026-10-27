<script lang="ts">
  import { goto } from "$app/navigation";
  import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
  import CircleStopIcon from "@lucide/svelte/icons/circle-stop";
  import FilterIcon from "@lucide/svelte/icons/filter";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import XIcon from "@lucide/svelte/icons/x";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { href } from "#lib/nav.js";
  import { FILTER_HELP, parseFilters } from "#lib/search/filters.js";
  import { pipeline } from "#lib/search/results.js";
  import { Results } from "#lib/search/results.svelte.js";
  import { scopeLabel, searches } from "#lib/search/searches.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { type ResultSort, searchOptions } from "#lib/settings.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { users } from "#lib/users.svelte.js";

  import ResponseCard from "./ResponseCard.svelte";
  import StatusIcon from "./StatusIcon.svelte";

  /** how many results show at first and how many more each press adds */
  const STEP = 5;
  const SORTS: { value: ResultSort; label: string }[] = [
    { value: "uploadSpeed", label: "Upload Speed (Fastest to Slowest)" },
    { value: "queueLength", label: "Queue Depth (Least to Most)" }
  ];

  // one search; the route mounts a fresh one for every id, so nothing here outlives it
  let { id }: { id: string } = $props();

  // svelte-ignore state_referenced_locally
  const results = new Results(id);
  $effect(() => () => results.close());

  keepCurrent(
    () => void searches.fetch(id),
    () => `search:${id}`
  );
  // the counts come with the list's frames
  $effect(() => live.retain("search"));

  const search = $derived(searches.list.get(id));
  let missing = $state(false);
  let working = $state(false);
  let shown = $state(STEP);
  let text = $state("");
  let filterText = $state("");
  let showBlocked = $state(false);
  let blocked = $state.raw<Set<string>>(new Set());
  const dismissed = new SvelteSet<string>();

  const canWrite = $derived(session.can("ReadWrite"));
  const connected = $derived(app.isServerConnected);
  const complete = $derived(!!search?.isComplete && !!search.endedAt);

  // a page opened before its frame arrived asks for the search; one that is gone goes back
  $effect(() => {
    if (search || missing) return;
    untrack(async () => {
      const outcome = await searches.fetch(id);
      if (outcome.ok) return;
      missing = true;
      if (outcome.status === 404) {
        toast("Search not found", "error");
        void goto(href("/searches"), { replaceState: true });
      }
    });
  });

  $effect(() => {
    if (!complete) return;
    const count = search?.responseCount ?? 0;
    untrack(() => void results.fetch(count));
  });

  // results from blocked users are hidden unless asked for
  void users.blocked().then((names) => {
    if (names) blocked = names;
  });

  // the filter applies a moment after typing stops
  $effect(() => {
    const typed = text;
    const timer = setTimeout(() => (filterText = typed), 150);
    return () => clearTimeout(timer);
  });

  const view = $derived(
    pipeline(results.results, {
      hidden: (u) => dismissed.has(u) || (!showBlocked && blocked.has(u)),
      filters: parseFilters(filterText),
      hideLocked: searchOptions.hideLocked,
      hideNoFreeSlots: searchOptions.hideNoFreeSlots,
      // results stay in the order they came while the search runs, so nothing jumps
      sort: results.stored ? searchOptions.sort : null
    })
  );
  const hiddenCount = $derived(results.results.length - view.length);
  const remaining = $derived(Math.max(view.length - shown, 0));

  const again = async () => {
    if (!search) return;
    working = true;
    const outcome = await searches.create(search.searchText, search.scope);
    working = false;
    if (outcome.ok) void goto(href(`/searches/${outcome.data.id}`));
    else toast(outcome.error, "error");
  };

  const stopOrDelete = async () => {
    if (!search) return;
    working = true;
    const done = search.isComplete;
    const outcome = done ? await searches.remove(id) : await searches.stop(id);
    working = false;
    if (!outcome.ok) toast(outcome.error, "error");
    else if (done) void goto(href("/searches"));
  };
</script>

{#if !search}
  {#if missing}
    <ErrorSegment
      caption="This search couldn't be loaded"
      retry={() => (missing = false)}
      retryLabel="Retry" />
  {:else}
    <div class="flex justify-center py-10"><Spinner class="size-8" /></div>
  {/if}
{:else}
  <div
    class="flex flex-wrap items-center gap-inline rounded-card border bg-surface p-4 shadow-card">
    <StatusIcon state={search.state} />
    <h2 class="min-w-0 flex-1 text-lg font-semibold wrap-anywhere">{search.searchText}</h2>
    {#if scopeLabel(search.scope)}<Badge tone="accent">{scopeLabel(search.scope)}</Badge>{/if}
    {#if canWrite}
      <div class="flex gap-inline">
        {#if results.stored}
          <Button variant="outline" disabled={!connected || working} onclick={() => void again()}>
            {#if working}<Spinner class="size-4" />{:else}<RefreshCwIcon />{/if}
            <span class="sm:max-lg:sr-only">Search Again</span>
          </Button>
        {/if}
        <Button variant="danger" disabled={working} onclick={() => void stopOrDelete()}>
          {#if search.isComplete}<Trash2Icon />{:else}<CircleStopIcon />{/if}
          <span class="sm:max-lg:sr-only">{search.isComplete ? "Delete" : "Stop"}</span>
        </Button>
      </div>
    {/if}
  </div>

  {#if !complete}
    <div class="flex items-center justify-center gap-2 py-4 text-sm text-fg-muted" role="status">
      <Spinner class="size-5" />
      {#if search.state === "InProgress"}
        Found {search.fileCount.toLocaleString()} files {search.lockedFileCount
          ? `(plus ${search.lockedFileCount.toLocaleString()} locked) `
          : ""}from {search.responseCount.toLocaleString()} users
      {:else}
        Loading results...
      {/if}
    </div>
  {:else if results.error}
    <ErrorSegment
      caption={results.error}
      retry={() => void results.fetch(search.responseCount)}
      retryLabel="Retry" />
  {:else if results.loading && !results.results.length}
    <div class="flex justify-center py-10"><Spinner class="size-8" /></div>
  {/if}

  {#if results.results.length}
    <div class="flex flex-col gap-3 rounded-card border bg-surface p-4 shadow-card">
      <div class="flex flex-wrap items-center gap-x-6 gap-y-3">
        <div class="flex items-center gap-2">
          <ArrowUpDownIcon class="size-4 text-fg-muted" />
          <Select bind:value={searchOptions.sort} options={SORTS} aria-label="Sort" class="w-72" />
        </div>
        <label class="flex items-center gap-2 text-sm">
          <Switch bind:checked={searchOptions.hideLocked} />Hide Locked Results
        </label>
        <label class="flex items-center gap-2 text-sm">
          <Switch bind:checked={searchOptions.hideNoFreeSlots} />Hide Results with No Free Slots
        </label>
        <label class="flex items-center gap-2 text-sm">
          <Switch bind:checked={searchOptions.fold} />Fold Results
        </label>
        {#if blocked.size}
          <label class="flex items-center gap-2 text-sm">
            <Switch bind:checked={showBlocked} />Show blocked
          </label>
        {/if}
      </div>
      <div class="relative">
        <FilterIcon
          class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fg-muted" />
        <input
          bind:value={text}
          type="search"
          aria-label="Filter"
          aria-describedby="filter-help"
          placeholder={FILTER_HELP}
          class="h-control w-full rounded-control border border-input bg-surface pr-9 pl-9 text-sm placeholder:text-fg-subtle focus-visible:border-accent" />
        {#if text}
          <button
            type="button"
            class="absolute top-1/2 right-3 -translate-y-1/2"
            aria-label="Clear the filter"
            onclick={() => (text = "")}>
            <XIcon class="size-4 text-danger" />
          </button>
        {/if}
      </div>
      <p id="filter-help" class="text-xs text-fg-muted">
        Words a path must have, <code>-word</code> to leave out, and <code>minbitrate:</code>,
        <code>minbitdepth:</code>, <code>minfilesize:</code> (bytes), <code>minlength:</code>
        (seconds),
        <code>minfilesinfolder:</code>, <code>iscbr</code>, <code>isvbr</code>,
        <code>islossless</code>, <code>islossy</code>. Files without the attribute are not filtered.
      </p>
    </div>

    <div class="flex flex-col gap-3" {@attach freezesWhileUsed}>
      {#each view.slice(0, shown) as result (result.key)}
        <ResponseCard
          {result}
          searchId={id}
          folded={searchOptions.fold}
          {connected}
          {canWrite}
          ondismiss={() => dismissed.add(result.response.username)} />
      {/each}
    </div>

    {#if remaining > 0}
      <Button size="default" class="h-11 w-full" onclick={() => (shown += STEP)}>
        Show {Math.min(STEP, remaining)} More Results ({remaining} remaining, {hiddenCount} hidden by
        filter(s))
      </Button>
    {:else if hiddenCount > 0}
      <Button variant="secondary" class="h-11 w-full" disabled>
        All results shown. {hiddenCount} results hidden by filter(s)
      </Button>
    {/if}
  {/if}
{/if}
