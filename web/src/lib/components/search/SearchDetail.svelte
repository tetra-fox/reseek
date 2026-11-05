<script lang="ts">
  import { goto } from "$app/navigation";
  import CircleStopIcon from "@lucide/svelte/icons/circle-stop";
  import PinIcon from "@lucide/svelte/icons/pin";
  import PinOffIcon from "@lucide/svelte/icons/pin-off";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import SearchXIcon from "@lucide/svelte/icons/search-x";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import RemoteFileInspector from "#lib/components/inspector/RemoteFileInspector.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { undoable } from "#lib/feedback/undo.js";
  import { asItem, enqueue } from "#lib/files/downloads.js";
  import { parseFilters } from "#lib/files/filters.js";
  import { plural } from "#lib/format.js";
  import { useInspector } from "#lib/inspector/inspector.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { href } from "#lib/nav.js";
  import { denseRows } from "#lib/search/dense.js";
  import type { Picked } from "#lib/search/facts.js";
  import { recent, togglePin } from "#lib/search/recent.svelte.js";
  import { pipeline } from "#lib/search/results.js";
  import { Results } from "#lib/search/results.svelte.js";
  import { openSearch, scopeLabel, searches } from "#lib/search/searches.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { LONG_QUEUE, searchOptions } from "#lib/settings.svelte.js";
  import { useCrumbs } from "#lib/shell/crumbs.svelte.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import DenseResults from "./DenseResults.svelte";
  import ResultsToolbar from "./ResultsToolbar.svelte";
  import ResultsTree from "./ResultsTree.svelte";
  import StatusIcon from "./StatusIcon.svelte";
  import Unreachable from "./Unreachable.svelte";

  // one search; the route mounts a fresh one for every id, so nothing here outlives it
  let { id }: { id: string } = $props();

  // svelte-ignore state_referenced_locally
  const results = new Results(id);
  $effect(() => () => results.close());

  keepCurrent(
    () => {
      if (!searches.starting.has(id)) void searches.fetch(id);
    },
    () => `search:${id}`
  );
  // the counts come with the list's frames
  $effect(() => live.retain("search"));

  /** the file the inspector shows, which follows focus in the results */
  let picked = $state.raw<Picked | null>(null);
  let dense = $state<DenseResults | null>(null);
  let tree = $state<ResultsTree | null>(null);
  useInspector(
    inspected,
    () => !!picked,
    () => (dense ?? tree)?.focus()
  );

  const search = $derived(searches.list.get(id));
  const refused = $derived(searches.refused.get(id));
  let missing = $state(false);
  let working = $state(false);
  let filterText = $state("");
  let typed = $state("");
  let showBlocked = $state(false);
  let blocked = $state.raw<Set<string>>(new Set());
  const dismissed = new SvelteSet<string>();
  /** whether a user's line is open unless toggled, which folding every result sets */
  let usersOpen = $state(!searchOptions.fold);
  const expansion = new Expansion(true, (key) => (key.startsWith("user:") ? usersOpen : undefined));

  useCrumbs(() => (search ? [{ label: search.searchText }] : []));

  const canWrite = $derived(session.can("ReadWrite"));
  const connected = $derived(app.isServerConnected);
  const complete = $derived(!!search?.isComplete && !!search.endedAt);
  const pinned = $derived(!!search && recent.pinned.includes(search.searchText));

  // a page opened before its frame arrived asks for the search; one that is gone goes back. One
  // still being asked for waits for the daemon's answer
  $effect(() => {
    if (search || missing || refused || searches.starting.has(id)) return;
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
    const text = typed;
    const timer = setTimeout(() => (filterText = text), 150);
    return () => clearTimeout(timer);
  });

  const view = $derived(
    pipeline(results.results, {
      hidden: (u) => dismissed.has(u) || (!showBlocked && blocked.has(u)),
      filters: parseFilters(filterText),
      hideLocked: searchOptions.hideLocked,
      hideNoFreeSlots: searchOptions.hideNoFreeSlots,
      offline: searchOptions.hideOffline ? (u) => users.get(u)?.presence === "Offline" : undefined,
      maxQueue: searchOptions.hideLongQueues ? LONG_QUEUE : null,
      // results stay in the order they came while the search runs, so nothing jumps
      sort: results.stored ? searchOptions.sort : null
    })
  );
  // what keeps results out of view, counted apart since each has its own way back: the ones
  // dismissed, those from blocked users, and the rest, which the filter and toggles hide
  const dismissedCount = $derived(
    results.results.filter((r) => dismissed.has(r.response.username)).length
  );
  const fromBlocked = $derived(
    showBlocked
      ? 0
      : results.results.filter(
          (r) => !dismissed.has(r.response.username) && blocked.has(r.response.username)
        ).length
  );
  const filteredCount = $derived(
    results.results.length - dismissedCount - fromBlocked - view.length
  );

  /** Empties the filter and turns off every toggle that hides results */
  const clearFilters = () => {
    typed = "";
    filterText = "";
    searchOptions.hideLocked = false;
    searchOptions.hideNoFreeSlots = false;
    searchOptions.hideOffline = false;
    searchOptions.hideLongQueues = false;
  };
  const rows = $derived(searchOptions.view === "dense" ? denseRows(view) : []);
  const counts = $derived({
    files: view.reduce((n, r) => n + r.response.fileCount + r.response.lockedFileCount, 0),
    users: view.length,
    allUsers: results.results.length,
    allFiles: results.results.reduce(
      (n, r) => n + r.response.fileCount + r.response.lockedFileCount,
      0
    )
  });

  const again = () => {
    if (!search) return;
    openSearch(search.searchText, search.scope);
  };

  const stop = async () => {
    working = true;
    const outcome = await searches.stop(id);
    working = false;
    if (!outcome.ok) toast(outcome.error, "error");
  };

  /** Deletes once its undo has passed; the list shows without it meanwhile */
  const remove = () => {
    searches.removeLater([id]);
    void goto(href("/searches"));
  };

  /** Enqueues the file the inspector shows and says what came of it */
  const downloadPicked = async ({ username, file }: Picked) => {
    const outcome = await enqueue({ username, files: [asItem(file)], searchId: id });
    toast(outcome.text, outcome.kind === "done" ? "info" : "error");
  };

  const dismiss = (username: string) => {
    dismissed.add(username);
    undoable(
      `Results from ${username} hidden`,
      () => {},
      () => dismissed.delete(username)
    );
  };
</script>

{#snippet inspected()}
  {#if picked}
    {@const shown = picked}
    <RemoteFileInspector
      file={shown.file}
      username={shown.username}
      facts={shown.facts}
      ondownload={canWrite && connected && !shown.file.locked
        ? () => void downloadPicked(shown)
        : undefined} />
  {/if}
{/snippet}

<!-- what is out of view and how to bring each part back -->
{#snippet hiddenLine()}
  <p class="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-sm text-fg-muted">
    {#if filteredCount}
      <span
        >{plural(filteredCount, "result", "results")} hidden by filters
        <Button variant="link" onclick={clearFilters}>Clear filters</Button></span>
    {/if}
    {#if dismissedCount}
      <span
        >{dismissedCount} dismissed
        <Button variant="link" onclick={() => dismissed.clear()}>Show again</Button></span>
    {/if}
    {#if fromBlocked}
      <span
        >{fromBlocked} from blocked users
        <Button variant="link" onclick={() => (showBlocked = true)}>Show</Button></span>
    {/if}
  </p>
{/snippet}

{#if refused}
  <ErrorSegment
    caption={`Could not start the search: ${refused}`}
    retry={again}
    retryLabel="Try again" />
{:else if !search}
  {#if missing}
    <ErrorSegment
      caption="Could not load this search"
      retry={() => (missing = false)}
      retryLabel="Retry" />
  {:else}
    <Skeleton class="h-16" />
    <Skeleton class="h-28" />
  {/if}
{:else}
  <div
    class="flex flex-wrap items-center gap-inline rounded-card border bg-surface p-4 shadow-card">
    <StatusIcon state={search.state} />
    <!-- the buttons go under the title rather than squeeze it below a word's width -->
    <h2 class="min-w-0 flex-1 basis-40 truncate text-lg font-semibold" title={search.searchText}>
      {search.searchText}
    </h2>
    {#if scopeLabel(search.scope)}<Badge tone="accent">{scopeLabel(search.scope)}</Badge>{/if}
    <div class="flex flex-wrap gap-inline">
      <Button
        variant="ghost"
        size="icon"
        aria-label={pinned ? "Unpin this search" : "Pin this search under the search box"}
        title={pinned ? "Unpin this search" : "Pin this search under the search box"}
        aria-pressed={pinned}
        onclick={() => togglePin(search.searchText)}>
        {#if pinned}<PinOffIcon />{:else}<PinIcon />{/if}
      </Button>
      {#if canWrite}
        <Button variant="outline" disabled={!connected || working} onclick={again}>
          <RefreshCwIcon />
          <span class="sm:max-lg:sr-only">Search again</span>
        </Button>
        {#if search.isComplete}
          <Button variant="danger" disabled={working} onclick={remove}>
            <Trash2Icon /><span class="sm:max-lg:sr-only">Delete</span>
          </Button>
        {:else}
          <Button variant="danger" disabled={working} onclick={() => void stop()}>
            <CircleStopIcon /><span class="sm:max-lg:sr-only">Stop</span>
          </Button>
        {/if}
      {/if}
    </div>
  </div>

  <Unreachable />

  {#if complete && results.error}
    <ErrorSegment
      caption={results.error}
      retry={() => void results.fetch(search.responseCount)}
      retryLabel="Retry" />
  {/if}

  {#if results.results.length}
    <ResultsToolbar
      bind:filter={typed}
      {counts}
      running={!complete}
      sortable={results.stored}
      blockedCount={blocked.size}
      bind:showBlocked
      onfold={(fold) => {
        usersOpen = !fold;
        expansion.forget((key) => key.startsWith("user:"));
      }} />

    {#if searchOptions.view === "dense"}
      {#if rows.length}
        <DenseResults
          bind:this={dense}
          {rows}
          searchId={id}
          canDownload={canWrite && connected}
          inspect={(file) => (picked = file)} />
      {/if}
    {:else if view.length}
      <ResultsTree
        bind:this={tree}
        {view}
        searchId={id}
        {expansion}
        {connected}
        {canWrite}
        ondismiss={dismiss}
        inspect={(file) => (picked = file)} />
    {/if}
    {#if !view.length}
      <EmptyState icon={SearchXIcon} title="All results are hidden">
        {#snippet action()}{@render hiddenLine()}{/snippet}
      </EmptyState>
    {:else if filteredCount || dismissedCount || fromBlocked}
      {@render hiddenLine()}
    {/if}
  {:else if !complete}
    <div class="flex items-center gap-2 text-sm text-fg-muted" role="status">
      <Spinner class="size-4" />
      {search.state === "InProgress" ? "Searching..." : "Starting search..."}
    </div>
    {#each [0, 1, 2] as n (n)}
      <div
        class="flex flex-col gap-2 rounded-card border bg-surface p-3 shadow-card"
        aria-hidden="true">
        <Skeleton class="h-4 w-48" />
        <Skeleton class="h-3 w-3/4" />
        <Skeleton class="h-3 w-2/3" />
      </div>
    {/each}
  {:else if results.loading}
    <Skeleton class="h-28" />
  {:else if !results.error}
    <EmptyState
      icon={SearchXIcon}
      title="No results"
      description="Use fewer or different words, or search again later." />
  {/if}
{/if}
