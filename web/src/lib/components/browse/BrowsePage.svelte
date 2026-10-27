<script lang="ts">
  import { goto } from "$app/navigation";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronUpIcon from "@lucide/svelte/icons/chevron-up";
  import CrosshairIcon from "@lucide/svelte/icons/crosshair";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";
  import { tick, untrack } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { browser } from "#lib/browse/browse.svelte.js";
  import { browseFilter } from "#lib/browse/filter.js";
  import { filesUnder } from "#lib/browse/tree.js";
  import FolderFiles from "#lib/components/files/FolderFiles.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatBytes } from "#lib/format.js";
  import { live } from "#lib/live/live.svelte.js";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";

  import DirectoryTree from "./DirectoryTree.svelte";
  import FileBrowser from "./FileBrowser.svelte";

  type Props = {
    /** a user to browse now, from a link */
    user: string | null;
    /** a folder of theirs to open once the browse is here */
    path: string | null;
    /** list every file under that folder */
    flat: boolean;
  };

  let { user, path, flat }: Props = $props();

  /** how often the progress is asked for while the stream can't push it */
  const POLL_MS = 500;

  let input = $state("");
  let field = $state<HTMLInputElement | null>(null);
  let typed = $state("");
  let query = $state("");
  let tree = $state<DirectoryTree | null>(null);
  let flatOpen = $state(false);

  const canWrite = $derived(session.can("ReadWrite"));
  const connected = $derived(app.isServerConnected);
  const filter = $derived(browseFilter(query, browser.separator));
  const node = $derived(browser.directory ? browser.tree.byName.get(browser.directory) : undefined);
  const flatNode = $derived(path ? browser.tree.byName.get(path) : undefined);

  // a link names a user: browse them unless their browse is what's shown, then open the folder
  $effect(() => {
    const wanted = user?.trim();
    untrack(async () => {
      if (!wanted) {
        await browser.restore();
        input = browser.username;
        return;
      }
      input = wanted;
      if (browser.username !== wanted || browser.phase !== "complete") {
        await browser.browse(wanted);
      }
      if (path && browser.tree.byName.has(path)) {
        await tick();
        await tree?.navigate(path);
        flatOpen = flat;
      }
    });
  });

  // the progress comes on the users topic, or by asking while the stream is down
  $effect(() => (browser.phase === "pending" ? live.retain("users") : undefined));
  $effect(() => {
    if (browser.phase !== "pending" || live.status === "connected") return;
    const timer = setInterval(() => void browser.poll(), POLL_MS);
    return () => clearInterval(timer);
  });

  // the tree filter applies a moment after typing stops
  $effect(() => {
    const text = typed;
    const timer = setTimeout(() => (query = text), 200);
    return () => clearTimeout(timer);
  });

  const start = () => void browser.browse(input);
  const clear = async () => {
    browser.clear();
    input = "";
    typed = "";
    if (user) await goto(href("/browse"), { replaceState: true });
    await tick();
    field?.focus();
  };

  const info = $derived(browser.info);

  const escapeClears = (node: HTMLElement) => {
    const keydown = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || browser.phase === "idle" || browser.phase === "pending") return;
      if ((e.target as HTMLElement).closest("[role=dialog],[role=alertdialog],[role=menu]")) return;
      void clear();
    };
    node.addEventListener("keydown", keydown);
    return () => node.removeEventListener("keydown", keydown);
  };
</script>

<!-- Escape clears the screen only while focus is in it, not in a dialog over it -->
<div class="flex flex-col gap-section" role="region" aria-label="Browse" {@attach escapeClears}>
  <form
    class="flex items-center gap-inline rounded-card border bg-surface p-3 shadow-card"
    onsubmit={(e) => {
      e.preventDefault();
      start();
    }}>
    <div class="relative min-w-0 flex-1">
      <FolderOpenIcon
        class="pointer-events-none absolute top-1/2 left-3 size-5 -translate-y-1/2 text-fg-muted" />
      <input
        bind:this={field}
        bind:value={input}
        type="search"
        data-lpignore="true"
        aria-label="Username"
        placeholder="Username"
        disabled={browser.phase !== "idle"}
        class="h-11 w-full rounded-control border border-input bg-surface pr-10 pl-10 text-base shadow-xs placeholder:text-fg-muted focus-visible:border-accent disabled:opacity-60" />
      {#if browser.phase === "pending"}
        <Spinner class="absolute top-1/2 right-3 size-4 -translate-y-1/2" label="Browsing" />
      {/if}
    </div>
    {#if browser.phase === "idle"}
      <Button
        type="submit"
        size="icon"
        class="size-11"
        aria-label="Browse"
        disabled={!input.trim()}>
        <SearchIcon />
      </Button>
    {:else if browser.phase !== "pending"}
      <Button
        variant="danger"
        size="icon"
        class="size-11"
        aria-label="Clear"
        onclick={() => void clear()}>
        <XIcon />
      </Button>
    {/if}
  </form>

  {#if browser.phase === "pending"}
    <div class="flex flex-col items-center gap-3 py-10" role="status">
      <Spinner class="size-10" />
      <p class="text-lg">
        Downloaded {Math.round(browser.progress?.percentComplete ?? 0)}% of Response
      </p>
    </div>
  {:else if browser.phase === "error"}
    <div class="mt-12 flex flex-col items-center gap-1 text-center">
      <p class="text-lg">Failed to browse {browser.username}</p>
      {#if browser.errorMessage}<p class="text-sm text-fg-muted">{browser.errorMessage}</p>{/if}
    </div>
  {:else if !browser.tree.roots.length}
    {#if browser.restoring}
      <div class="flex flex-col items-center gap-3 py-10" role="status">
        <Spinner class="size-10" />
        <p class="text-lg">Loading saved results</p>
      </div>
    {:else if browser.phase === "complete"}
      <EmptyState icon={FolderOpenIcon} title="User is not sharing any files" />
    {/if}
  {:else}
    <section class="flex flex-col rounded-card border bg-surface shadow-card">
      <header class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 pt-3">
        <Username name={browser.username} />
        {#if info}
          <span class="text-sm text-fg-muted">
            {(info.files + info.lockedFiles).toLocaleString()} files in {(
              info.directories + info.lockedDirectories
            ).toLocaleString()} directories (including {info.lockedFiles.toLocaleString()} files in
            {info.lockedDirectories.toLocaleString()} locked directories), {formatBytes(info.bytes)}
          </span>
        {/if}
        {#if canWrite}
          <Button
            variant="ghost"
            size="sm"
            class="ml-auto"
            href={href(`/searches?user=${encodeURIComponent(browser.username)}`)}>
            <SearchIcon />Search this user
          </Button>
        {/if}
      </header>
      <div class="flex flex-wrap items-center gap-inline px-4 py-3">
        <div class="relative min-w-48 flex-1">
          <input
            bind:value={typed}
            type="search"
            aria-label="Filter"
            placeholder="lackluster container ext:flac minbr:320"
            class="h-control w-full rounded-control border border-input bg-surface pr-9 pl-3 text-sm placeholder:text-fg-subtle focus-visible:border-accent" />
          {#if typed}
            <button
              type="button"
              class="absolute top-1/2 right-3 -translate-y-1/2"
              aria-label="Clear the filter"
              onclick={() => (typed = "")}>
              <XIcon class="size-4 text-danger" />
            </button>
          {/if}
        </div>
        <Button variant="outline" size="sm" onclick={() => tree?.expandAll()}>
          <ChevronDownIcon />Expand all
        </Button>
        <Button variant="outline" size="sm" onclick={() => tree?.collapseAll()}>
          <ChevronUpIcon />Collapse all
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!browser.directory}
          onclick={() => {
            typed = "";
            query = "";
            if (browser.directory) void tree?.navigate(browser.directory);
          }}>
          <CrosshairIcon />Show selected directory
        </Button>
      </div>
      <div class="border-t">
        <DirectoryTree
          bind:this={tree}
          tree={browser.tree}
          separator={browser.separator}
          selected={browser.directory}
          onselect={(name: string) => name !== browser.directory && browser.select(name)}
          {filter} />
      </div>
    </section>
    {#if node}
      <FileBrowser
        {node}
        username={browser.username}
        {filter}
        {canWrite}
        {connected}
        onnavigate={(name) => {
          typed = "";
          query = "";
          void tree?.navigate(name);
        }} />
    {/if}
  {/if}
</div>

{#if flatNode}
  <FolderFiles
    bind:open={flatOpen}
    username={browser.username}
    folder={flatNode.name}
    files={filesUnder(flatNode, browser.separator)}
    {connected} />
{/if}
