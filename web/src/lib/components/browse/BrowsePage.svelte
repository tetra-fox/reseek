<script lang="ts">
  import { goto } from "$app/navigation";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronUpIcon from "@lucide/svelte/icons/chevron-up";
  import CrosshairIcon from "@lucide/svelte/icons/crosshair";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import HistoryIcon from "@lucide/svelte/icons/history";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";
  import { tick, untrack } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { browser } from "#lib/browse/browse.svelte.js";
  import { browseFilter } from "#lib/browse/filter.js";
  import { browsed, forgetBrowsed, restoreBrowsed } from "#lib/browse/recent.svelte.js";
  import { filesUnder } from "#lib/browse/tree.js";
  import FolderFiles from "#lib/components/files/FolderFiles.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { useCrumbs } from "#lib/crumbs.svelte.js";
  import { formatBytes } from "#lib/format.js";
  import { keepsKey, layerOpen, withModifier } from "#lib/keys.js";
  import { live } from "#lib/live/live.svelte.js";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { undoable } from "#lib/undo.js";

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

  /** Shows a folder of the listing: the tree opens to it and the selection card shows it */
  const show = (name: string) => {
    typed = "";
    query = "";
    void tree?.navigate(name);
  };

  // the user, then each folder down to the one open; a step that is a listed folder opens it
  useCrumbs(() => {
    if (!browser.username || browser.phase === "idle") return [];
    const owner = { label: browser.username, onclick: () => browser.select(null) };
    const directory = browser.directory;
    if (!directory) return [owner];
    const sep = browser.separator;
    const parts = directory.split(sep);
    return [
      owner,
      ...parts.map((part, i) => {
        const path = parts.slice(0, i + 1).join(sep);
        const last = i === parts.length - 1;
        return {
          label: part,
          onclick: !last && browser.tree.byName.has(path) ? () => show(path) : undefined,
          copy: last ? directory : undefined
        };
      })
    ];
  });

  const forget = (username: string) => {
    const at = forgetBrowsed(username);
    undoable(
      `Removed ${username} from recently browsed`,
      () => {},
      () => restoreBrowsed(username, at)
    );
  };

  const escapeClears = (node: HTMLElement) => {
    const keydown = (e: KeyboardEvent) => {
      // Escape in the filter field clears what was typed there, not the browse
      if (e.key !== "Escape" || withModifier(e) || keepsKey(e)) return;
      if (browser.phase === "idle" || browser.phase === "pending") return;
      // a card, menu or dialog open over the screen closes first
      if (layerOpen()) return;
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
      <Input
        bind:ref={field}
        bind:value={input}
        type="search"
        icon={FolderOpenIcon}
        data-lpignore="true"
        aria-label="Username"
        placeholder="Username"
        disabled={browser.phase !== "idle"}
        class="h-11 pl-10 text-base md:text-base" />
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

  {#if browser.phase === "idle" && !browser.restoring}
    {#if browsed.users.length}
      <section class="flex flex-col gap-2" aria-label="Recently browsed">
        <h2 class="flex items-center gap-1.5 text-sm text-fg-muted">
          <HistoryIcon class="size-4" />Recently browsed
        </h2>
        <ul class="flex flex-wrap gap-1.5">
          {#each browsed.users as name (name)}
            <li class="group flex items-center rounded-full border bg-surface shadow-xs">
              <button
                type="button"
                class="py-1 pl-3 text-sm hover:text-accent"
                onclick={() => {
                  input = name;
                  start();
                }}>{name}</button>
              <button
                type="button"
                class="rounded-full p-1.5 text-fg-muted row-action"
                aria-label={`Remove ${name} from recently browsed`}
                onclick={() => forget(name)}>
                <XIcon class="size-3" />
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {:else}
      <EmptyState
        icon={FolderOpenIcon}
        title="Browse someone's shares"
        description="Type a username above and press Enter to see every folder they share; the users you browse are listed here next time." />
    {/if}
  {/if}

  {#if browser.phase === "pending"}
    {@const percent = Math.round(browser.progress?.percentComplete ?? 0)}
    <div class="flex flex-col items-center gap-3 py-10" role="status">
      <p class="text-lg">Downloaded {percent}% of Response</p>
      <div
        class="h-1.5 w-64 overflow-hidden rounded-full bg-surface-muted ring-1 ring-border"
        role="progressbar"
        aria-label="Browse progress"
        aria-valuenow={percent}
        aria-valuemin={0}
        aria-valuemax={100}>
        <div
          class="h-full bg-accent transition-[width] duration-(--motion-slow)"
          style:width={`${percent}%`}>
        </div>
      </div>
      <div class="mt-4 flex w-full max-w-xl flex-col gap-2" aria-hidden="true">
        {#each [0, 1, 2, 3, 4] as n (n)}
          <Skeleton class={`h-5 ${["", "ml-6", "ml-12"][n % 3]}`} />
        {/each}
      </div>
    </div>
  {:else if browser.phase === "error"}
    <div class="mt-12 flex flex-col items-center gap-1 text-center">
      <p class="text-lg">Failed to browse {browser.username}</p>
      {#if browser.errorMessage}<p class="text-sm text-fg-muted">{browser.errorMessage}</p>{/if}
    </div>
  {:else if !browser.tree.roots.length}
    {#if browser.restoring}
      <div class="flex flex-col items-center gap-3 py-10" role="status">
        <Spinner class="size-6" />
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
        <div class="min-w-48 flex-1">
          <Input
            bind:value={typed}
            type="search"
            aria-label="Filter"
            placeholder="lackluster container type:flac bitrate:320" />
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
        onnavigate={show} />
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
