<script lang="ts">
  import { goto } from "$app/navigation";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronUpIcon from "@lucide/svelte/icons/chevron-up";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import HistoryIcon from "@lucide/svelte/icons/history";
  import LinkIcon from "@lucide/svelte/icons/link";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";
  import { untrack } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { browser } from "#lib/browse/browse.svelte.js";
  import { browseFilter } from "#lib/browse/filter.js";
  import { browsed, forgetBrowsed, restoreBrowsed } from "#lib/browse/recent.svelte.js";
  import FileCells from "#lib/components/files/FileCells.svelte";
  import FileHeader from "#lib/components/files/FileHeader.svelte";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import FolderLine from "#lib/components/files/FolderLine.svelte";
  import SelectionBar from "#lib/components/files/SelectionBar.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { undoable } from "#lib/feedback/undo.js";
  import { fileColumns, fileTemplate } from "#lib/files/columns.js";
  import {
    asItem,
    downloadPlan,
    type Enqueued,
    enqueue,
    followDownloads
  } from "#lib/files/downloads.js";
  import { slskLink } from "#lib/files/slsk.js";
  import { formatBytes, getFileName, plural } from "#lib/format.js";
  import { live } from "#lib/live/live.svelte.js";
  import { href } from "#lib/nav.js";
  import { FILTER_PLACEHOLDER } from "#lib/search/filters.js";
  import { session } from "#lib/session.svelte.js";
  import { useCrumbs } from "#lib/shell/crumbs.svelte.js";
  import {
    ancestors,
    type FolderNode,
    itemsUnder,
    parentOf,
    type RemoteFile,
    tickedCounts,
    treeLines
  } from "#lib/tree/folders.js";
  import { keepsKey, layerOpen, withModifier } from "#lib/ui/keys.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  type Props = {
    /** a user to browse now, from a link */
    user: string | null;
    /** a folder of theirs to go to once the browse is here */
    path: string | null;
  };

  let { user, path }: Props = $props();

  /** how often the progress is asked for while the stream can't push it */
  const POLL_MS = 500;

  let input = $state("");
  let field = $state<HTMLInputElement | null>(null);
  let typed = $state("");
  let query = $state("");
  let tree = $state<ReturnType<typeof FileTree<RemoteFile>> | null>(null);

  const canWrite = $derived(session.can("ReadWrite"));
  const connected = $derived(app.isServerConnected);
  const write = $derived(canWrite && connected);
  const filter = $derived(browseFilter(query));
  const expansion = browser.expansion;
  const lines = $derived(
    treeLines(browser.tree.roots, expansion.isOpen, (f) => f.filename, filter)
  );
  const columns = $derived(fileColumns({ quality: true, phone: viewport.phone }));

  followDownloads();

  /** Opens the folders down to `target` and the folder itself, and goes to its line */
  const navigate = (target: string) => {
    if (!browser.tree.byPath.has(target)) return;
    typed = "";
    query = "";
    for (const above of ancestors(target, browser.tree, browser.separator)) {
      expansion.set(above, true);
    }
    expansion.set(target, true);
    void tree?.navigate(`folder:${target}`);
  };

  // a link names a user: browse them unless their browse is what's shown, then go to the folder
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
      if (path) browser.target = path;
    });
  });
  // a folder to go to waits until the tree is drawn
  $effect(() => {
    const target = browser.target;
    if (!target || !tree) return;
    untrack(() => {
      browser.target = null;
      navigate(target);
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

  // the open folders and the ticked files are kept a moment after they change
  $effect(() => {
    void expansion.snapshot();
    void [...browser.selected];
    const timer = setTimeout(browser.save, 300);
    return () => clearTimeout(timer);
  });

  const start = () => void browser.browse(input);
  const clear = async () => {
    browser.clear();
    input = "";
    typed = "";
    if (user) await goto(href("/browse"), { replaceState: true });
    field?.focus();
  };

  const info = $derived(browser.info);

  // the user, then each folder down to the line with focus; a folder step goes to its line
  useCrumbs(() => {
    if (!browser.username || browser.phase === "idle") return [];
    const owner = { label: browser.username };
    const line = tree?.focused();
    if (!line) return [owner];
    const at = line.kind === "folder" ? line.node.path : line.folder.path;
    const sep = browser.separator;
    const parts = at.split(sep);
    return [
      owner,
      ...parts.map((part, i) => {
        const step = parts.slice(0, i + 1).join(sep);
        const last = i === parts.length - 1;
        return {
          label: part,
          onclick: !last && browser.tree.byPath.has(step) ? () => navigate(step) : undefined,
          copy: last ? at : undefined
        };
      })
    ];
  });

  /** each folder's files by path, made for a folder the first time one of its files is ticked */
  const indexes = $derived.by(() => {
    void browser.tree;
    return new Map<string, Map<string, RemoteFile>>();
  });
  /** the ticked files, found in their folders */
  const chosen = $derived(
    [...browser.selected].flatMap((at) => {
      const folder = parentOf(at, browser.separator);
      let index = indexes.get(folder);
      if (!index) {
        const items = browser.tree.byPath.get(folder)?.items ?? [];
        index = new Map(items.map((f) => [f.filename, f]));
        indexes.set(folder, index);
      }
      return index.get(at) ?? [];
    })
  );
  const ticked = $derived(tickedCounts(browser.selected, (p) => p));

  /** the files under a folder the filter keeps, each folder's worked out once per filter */
  const kept = $derived.by(() => {
    const known = new Map<FolderNode<RemoteFile>, RemoteFile[]>();
    return (node: FolderNode<RemoteFile>) => {
      let found = known.get(node);
      if (!found) {
        found = filter ? itemsUnder(node).filter((f) => !f.locked && filter.item(f)) : [];
        known.set(node, found);
      }
      return found;
    };
  });

  /** A folder's tick over the files under it that can be ticked, those the filter keeps */
  const tickOf = (node: FolderNode<RemoteFile>) => {
    if (filter) {
      const under = kept(node);
      const on = under.filter((f) => browser.selected.has(f.filename)).length;
      return { on, of: under.length, files: () => under };
    }
    const on = ticked.get(node.path) ?? 0;
    return { on, of: node.unlocked, files: () => itemsUnder(node).filter((f) => !f.locked) };
  };

  /** The top folder a path is under: the listing's folder at the top of its branch */
  const topOf = (at: string) =>
    ancestors(at, browser.tree, browser.separator)[0] ?? getFileName(at);

  /**
   * Downloads files: each top folder's as batches that keep their folders below the deepest one
   * holding them. The first answer that is not a full success stops the rest
   */
  const download = async (list: RemoteFile[]): Promise<Enqueued> => {
    let last: Enqueued = { kind: "done", text: "" };
    for (const group of downloadPlan(list, topOf, browser.separator)) {
      last = await enqueue({
        username: browser.username,
        files: group.files.map((f) => asItem(f)),
        destination: group.destination ?? undefined
      });
      if (last.kind !== "done") break;
    }
    return last;
  };

  /** Every file under a folder, filtered or not, as one batch in a folder named like it */
  const downloadFolder = async (node: FolderNode<RemoteFile>) => {
    const outcome = await enqueue({
      username: browser.username,
      files: itemsUnder(node)
        .filter((f) => !f.locked)
        .map((f) => asItem(f)),
      destination: node.label
    });
    toast(outcome.text, outcome.kind === "done" ? "info" : "error");
  };

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
      // a card, menu or dialog open over the screen closes first, and a selection is cleared
      // before the browse
      if (layerOpen() || browser.selected.size) return;
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
      <EmptyState icon={FolderOpenIcon} title="Browse a user's shares" />
    {/if}
  {/if}

  {#if browser.phase === "pending"}
    {@const percent = Math.round(browser.progress?.percentComplete ?? 0)}
    <div class="flex flex-col items-center gap-3 py-10" role="status">
      <p class="text-lg">Loading shares... {percent}%</p>
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
      <p class="text-lg">Could not browse {browser.username}</p>
      {#if browser.errorMessage}<p class="text-sm text-fg-muted">{browser.errorMessage}</p>{/if}
    </div>
  {:else if !browser.tree.roots.length}
    {#if browser.restoring}
      <div class="flex flex-col items-center gap-3 py-10" role="status">
        <Spinner class="size-6" />
        <p class="text-lg">Loading saved shares...</p>
      </div>
    {:else if browser.phase === "complete"}
      <EmptyState icon={FolderOpenIcon} title={`${browser.username} is not sharing any files`} />
    {/if}
  {:else}
    <section class="flex flex-col rounded-card border bg-surface shadow-card">
      <header class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 pt-3">
        <Username name={browser.username} />
        {#if info}
          <span class="text-sm text-fg-muted">
            {plural(info.files + info.lockedFiles, "file", "files")} in {plural(
              info.directories + info.lockedDirectories,
              "folder",
              "folders"
            )}, {formatBytes(info.bytes)}{info.lockedFiles
              ? ` (${plural(info.lockedFiles, "file", "files")} in ${plural(info.lockedDirectories, "folder", "folders")} ${info.lockedFiles === 1 ? "is" : "are"} locked)`
              : ""}
          </span>
        {/if}
        {#if canWrite}
          <Button
            variant="ghost"
            size="sm"
            class="ml-auto"
            href={href(`/searches?user=${encodeURIComponent(browser.username)}`)}>
            <SearchIcon />Search this user's shares
          </Button>
        {/if}
      </header>
      <div class="flex flex-wrap items-center gap-inline px-4 py-3">
        <div class="min-w-48 flex-1">
          <Input
            bind:value={typed}
            type="search"
            aria-label="Filter"
            placeholder={FILTER_PLACEHOLDER} />
        </div>
        <Button variant="outline" size="sm" onclick={() => expansion.reset(true)}>
          <ChevronDownIcon />Expand all
        </Button>
        <Button variant="outline" size="sm" onclick={() => expansion.reset(false)}>
          <ChevronUpIcon />Collapse all
        </Button>
      </div>
    </section>
    <FileTree
      bind:this={tree}
      {lines}
      {expansion}
      label="Shared folders"
      columns={fileTemplate(columns)}
      height="max(20rem, calc(var(--content-h) - 16rem))"
      selected={browser.selected}
      idOf={(f) => f.filename}
      tickable={(f) => !f.locked}
      nameOf={(line) =>
        line.kind === "folder" ? line.node.label : getFileName(line.item.filename)}
      onopen={write ? (f) => void download([f]) : undefined}
      onmiddleclick={write ? (f) => void download([f]) : undefined}>
      {#snippet header()}
        <FileHeader {columns} />
      {/snippet}
      {#snippet folder(node, depth)}
        {@const state = tickOf(node)}
        <FolderLine
          {node}
          open={!!filter || expansion.isOpen(node.path)}
          {depth}
          tick={state.of
            ? {
                checked: state.on === state.of,
                indeterminate: state.on > 0 && state.on < state.of,
                ontick: (on) =>
                  browser.tick(
                    state.files().map((f) => f.filename),
                    on
                  )
              }
            : undefined}
          ondownload={write && node.unlocked ? () => void downloadFolder(node) : undefined}>
          {#snippet menu()}
            <Menu.Item onSelect={() => void copyText(node.path)}><CopyIcon />Copy path</Menu.Item>
            <Menu.Item onSelect={() => void copyText(slskLink(browser.username, node.path, true))}
              ><LinkIcon />Copy slsk:// link</Menu.Item>
          {/snippet}
        </FolderLine>
      {/snippet}
      {#snippet item(file, _depth, tick)}
        <FileCells
          {file}
          username={browser.username}
          {columns}
          selected={browser.selected.has(file.filename)}
          ontick={tick}
          ondownload={write && !file.locked ? () => void download([file]) : undefined} />
      {/snippet}
    </FileTree>
    {#if chosen.length}
      <SelectionBar
        files={chosen}
        ondownload={write ? () => download(chosen) : undefined}
        onclear={() => browser.selected.clear()} />
    {/if}
  {/if}
</div>
