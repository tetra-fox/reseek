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
  import FolderFiles from "#lib/components/files/FolderFiles.svelte";
  import RemoteFileInspector from "#lib/components/inspector/RemoteFileInspector.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { undoable } from "#lib/feedback/undo.js";
  import { AllFiles } from "#lib/files/allfiles.svelte.js";
  import { asItem, enqueue } from "#lib/files/downloads.js";
  import { parseFilters } from "#lib/files/filters.js";
  import { formatCount, listText, plural } from "#lib/format.js";
  import { useInspector } from "#lib/inspector/inspector.svelte.js";
  import { type ActiveFilter, clearThen, typedFilter } from "#lib/lists/filtered.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { href } from "#lib/nav.js";
  import type { Picked } from "#lib/search/facts.js";
  import { resultRows } from "#lib/search/list.js";
  import { recent, togglePin } from "#lib/search/recent.svelte.js";
  import { HIDING, type HideKey, pipeline, type Result } from "#lib/search/results.js";
  import { Results } from "#lib/search/results.svelte.js";
  import {
    isDone,
    openSearch,
    scopeLabel,
    searches,
    searchStage
  } from "#lib/search/searches.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import {
    HIDE_DEFAULTS,
    LONG_QUEUE,
    type ResultSort,
    searchOptions
  } from "#lib/settings.svelte.js";
  import { useCrumbs } from "#lib/shell/crumbs.svelte.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import ResultsList from "./ResultsList.svelte";
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
  // the counts come with the list's frames, and blocks made meanwhile with the users' frames
  $effect(() => live.retain("search"));
  $effect(() => live.retain("users"));

  /** the file focus last came to in the results, which the inspector shows */
  let picked = $state.raw<Picked | null>(null);
  let list = $state<ResultsList | null>(null);
  let tree = $state<ResultsTree | null>(null);
  useInspector(
    inspected,
    () => !!shown,
    () => (list ?? tree)?.focus()
  );
  /** the dialog both layouts open on every file under a folder */
  const allFiles = new AllFiles();

  const search = $derived(searches.list.get(id));
  const refused = $derived(searches.refused.get(id));
  let missing = $state(false);
  let working = $state(false);
  let filterText = $state("");
  let typed = $state("");
  let showBlocked = $state(false);
  /** the users blocked now, read once and kept by the users' frames */
  const blocked = new SvelteSet<string>();
  const dismissed = new SvelteSet<string>();
  /** the ticked files, kept across the layouts, each by its result's key and its path */
  const selected = new SvelteSet<string>();
  const expansion = new Expansion(true);

  useCrumbs(() => (search ? [{ label: search.searchText }] : []));

  const canWrite = $derived(session.can("ReadWrite"));
  const connected = $derived(app.isServerConnected);
  const complete = $derived(!!search && isDone(search.state));
  const running = $derived(!!search && !complete);
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
        void goto(href("/searches"), { replace: true });
      }
    });
  });

  // the responses a running search took before the page opened; those it takes until the stream
  // follows it are read again once the stream greets
  $effect(() => {
    if (running) untrack(() => void results.catchUp());
  });

  $effect(() => {
    if (!complete) return;
    const count = search?.responseCount ?? 0;
    untrack(() => void results.fetch(count));
  });

  // results from blocked users are hidden unless asked for. A frame tells of a block or an
  // unblock made since, and outranks the read, which may have been answered before it
  const framed = new Set<string>();
  const blockChanged = (username: string, isBlocked: boolean) => {
    framed.add(username);
    if (isBlocked) blocked.add(username);
    else blocked.delete(username);
  };
  $effect(() => live.on("users.updated", (user) => blockChanged(user.username, !!user.block)));
  $effect(() => live.on("users.removed", ({ username }) => blockChanged(username, false)));
  void users.blocked().then((names) => {
    for (const name of names ?? []) if (!framed.has(name)) blocked.add(name);
  });

  // the filter applies a moment after typing stops
  $effect(() => {
    const text = typed;
    const timer = setTimeout(() => (filterText = text), 150);
    return () => clearTimeout(timer);
  });

  /**
   * the toggles turned on beyond how they start, and the filter text: what the line of filters
   * names and Clear filters turns off. A toggle on from the start shows only as its pressed chip
   */
  const filters = $derived<ActiveFilter[]>([
    ...HIDING.filter((h) => searchOptions[h.key] && !HIDE_DEFAULTS[h.key]).map((h) => ({
      id: h.key,
      label: h.label,
      clear: () => (searchOptions[h.key] = false)
    })),
    ...typedFilter(filterText, () => {
      typed = "";
      filterText = "";
    })
  ]);
  const anyNamed = $derived(filters.length > 0);
  /** whether a toggle is on from the start and still on, so the line of filters leaves it out */
  const onByDefault = (key: HideKey) => searchOptions[key] && HIDE_DEFAULTS[key];

  /** the pipeline's options with the toggles `on` holds for and the filter `text` */
  const options = (on: (key: HideKey) => boolean, text: string, sort: ResultSort | null) => ({
    hidden: (u: string) => dismissed.has(u) || (!showBlocked && blocked.has(u)),
    filters: parseFilters(text),
    hideLocked: on("hideLocked"),
    hideNoFreeSlots: on("hideNoFreeSlots"),
    offline: on("hideOffline") ? (u: string) => users.get(u)?.presence === "Offline" : undefined,
    maxQueue: on("hideLongQueues") ? LONG_QUEUE : null,
    sort
  });
  const view = $derived(
    pipeline(
      results.results,
      // results stay in the order they came while the search runs, so nothing jumps
      options((key) => searchOptions[key], filterText, results.stored ? searchOptions.sort : null)
    )
  );
  /** the picked file while the filters leave it in the results */
  const shown = $derived.by(() => {
    if (!picked) return null;
    const { username, file } = picked;
    const listed = (r: Result) =>
      r.response.username === username &&
      (file.locked ? r.response.lockedFiles : r.response.files).some(
        (f) => f.filename === file.filename
      );
    return view.some(listed) ? picked : null;
  });
  /** the results with the named filters off, which the line of filters counts the shown ones of */
  const unnamed = $derived(
    anyNamed ? pipeline(results.results, options(onByDefault, "", null)) : view
  );
  // what keeps results out of view, counted apart since each has its own way back: the ones
  // dismissed, those from blocked users, those the toggles on from the start hide, and those the
  // named filters hide
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
  /** the toggles on from the start and still on */
  const defaults = $derived(HIDING.filter((h) => onByDefault(h.key)));
  const byDefaults = $derived(
    results.results.length - dismissedCount - fromBlocked - unnamed.length
  );
  const filteredCount = $derived(unnamed.length - view.length);

  /** gives focus to the results once the filters are off */
  const refocus = () => (list ?? tree)?.focus();
  const rows = $derived(searchOptions.layout === "list" ? resultRows(view) : []);
  const fileCount = (list: Result[]) =>
    list.reduce((n, r) => n + r.response.fileCount + r.response.lockedFileCount, 0);
  const counts = $derived({
    files: fileCount(view),
    users: view.length,
    allUsers: results.results.length,
    allFiles: fileCount(results.results)
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
  {#if shown}
    {@const offer = shown}
    <RemoteFileInspector
      file={offer.file}
      username={offer.username}
      facts={offer.facts}
      ondownload={canWrite && connected && !offer.file.locked
        ? () => void downloadPicked(offer)
        : undefined} />
  {/if}
{/snippet}

{#snippet searchAgain()}
  <Button variant="outline" disabled={!connected} onclick={again}>
    <RefreshCwIcon />Search again
  </Button>
{/snippet}

<!-- what is out of view and how to bring each part back -->
{#snippet hiddenLine()}
  <p class="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-sm text-fg-muted">
    {#if filteredCount}
      <span
        >{plural(filteredCount, "result", "results")} hidden by filters
        <Button variant="link" onclick={() => void clearThen(filters, undefined, refocus)}
          >Clear filters</Button
        ></span>
    {/if}
    {#if dismissedCount}
      <span
        >{formatCount(dismissedCount)} dismissed
        <Button variant="link" onclick={() => dismissed.clear()}>Show again</Button></span>
    {/if}
    {#if byDefaults}
      <span
        >{plural(byDefaults, "result", "results")} hidden by {listText(defaults.map((h) => h.chip))}
        <Button
          variant="link"
          onclick={() => {
            for (const h of defaults) searchOptions[h.key] = false;
          }}>Show</Button
        ></span>
    {/if}
    {#if fromBlocked}
      <span
        >{formatCount(fromBlocked)} from blocked users
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
  <div class="flex flex-wrap items-center gap-inline">
    <StatusIcon state={search.state} />
    <!-- the buttons go under the title rather than squeeze it below a word's width -->
    <Tooltip text={search.searchText} clipped>
      {#snippet child({ props })}
        <h2
          {...props}
          dir="auto"
          class="line-clamp-2 min-w-0 flex-1 basis-40 text-2xl font-semibold tracking-tight wrap-anywhere">
          {search.searchText}
        </h2>
      {/snippet}
    </Tooltip>
    {#if scopeLabel(search.scope)}
      {@const scope = scopeLabel(search.scope)}
      <Tooltip text={scope ?? undefined} clipped>
        {#snippet child({ props })}
          <Badge {...props} tone="accent">{scope}</Badge>
        {/snippet}
      </Tooltip>
    {/if}
    <div class="flex flex-wrap gap-inline">
      <Tooltip text={pinned ? "Unpin search" : "Pin search"}>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            aria-label={pinned ? "Unpin search" : "Pin search"}
            aria-pressed={pinned}
            onclick={() => togglePin(search.searchText)}>
            {#if pinned}<PinOffIcon />{:else}<PinIcon />{/if}
          </Button>
        {/snippet}
      </Tooltip>
      {#if canWrite}
        <!-- between sm and lg these show only their icons, with the words in a tooltip -->
        {@const iconsOnly = viewport.sm && !viewport.lg}
        <Tooltip text={iconsOnly ? "Search again" : undefined}>
          {#snippet child({ props })}
            <Button {...props} variant="outline" disabled={!connected || working} onclick={again}>
              <RefreshCwIcon />
              <span class="sm:max-lg:sr-only">Search again</span>
            </Button>
          {/snippet}
        </Tooltip>
        {#if complete}
          <Tooltip text={iconsOnly ? "Delete" : undefined}>
            {#snippet child({ props })}
              <Button {...props} variant="danger" disabled={working} onclick={remove}>
                <Trash2Icon /><span class="sm:max-lg:sr-only">Delete</span>
              </Button>
            {/snippet}
          </Tooltip>
        {:else}
          <Tooltip text={iconsOnly ? "Stop" : undefined}>
            {#snippet child({ props })}
              <Button {...props} variant="danger" disabled={working} onclick={() => void stop()}>
                <CircleStopIcon /><span class="sm:max-lg:sr-only">Stop</span>
              </Button>
            {/snippet}
          </Tooltip>
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
      {filters}
      running={!complete}
      sortable={results.stored}
      blockedCount={blocked.size}
      bind:showBlocked
      {expansion} />
    <Filter.Summary
      {filters}
      shown={counts.files}
      total={fileCount(unnamed)}
      noun={["file", "files"]}
      {refocus} />

    {#if searchOptions.layout === "list"}
      {#if rows.length}
        <ResultsList
          bind:this={list}
          {rows}
          searchId={id}
          canDownload={canWrite && connected}
          inspect={(file) => (picked = file)}
          ondismiss={dismiss}
          {allFiles}
          {selected} />
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
        inspect={(file) => (picked = file)}
        {allFiles}
        {selected} />
    {/if}
    {#if !view.length}
      <EmptyState icon={SearchXIcon} title="All results are hidden">
        {#snippet action()}{@render hiddenLine()}{/snippet}
      </EmptyState>
    {:else if filteredCount || dismissedCount || fromBlocked || byDefaults}
      {@render hiddenLine()}
    {/if}
  {:else if !complete}
    <div class="flex items-center gap-2 text-sm text-fg-muted" role="status">
      <Spinner class="size-4" />
      {search.state === "InProgress"
        ? "Searching..."
        : search.state === "Queued"
          ? "Waiting for another search to finish"
          : "Starting search..."}
    </div>
    {#each [0, 1, 2] as n (n)}
      <div class="flex flex-col gap-2 rounded-card border bg-surface p-3" aria-hidden="true">
        <Skeleton class="h-4 w-48" />
        <Skeleton class="h-3 w-3/4" />
        <Skeleton class="h-3 w-2/3" />
      </div>
    {/each}
  {:else if results.loading}
    <Skeleton class="h-28" />
  {:else if searchStage(search.state) === "errored"}
    <EmptyState
      icon={SearchXIcon}
      title="The search failed"
      action={canWrite ? searchAgain : undefined} />
  {:else if !results.error}
    <EmptyState
      icon={SearchXIcon}
      title="No results"
      description="Try fewer or different words, or search again later" />
  {/if}
{/if}

{#if allFiles.shown}
  <FolderFiles
    bind:open={allFiles.open}
    username={allFiles.shown.username}
    folder={allFiles.shown.folder}
    files={allFiles.shown.files}
    {connected} />
{/if}
