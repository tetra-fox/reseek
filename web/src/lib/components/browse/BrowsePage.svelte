<script lang="ts">
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import { untrack } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { browser } from "#lib/browse/browse.svelte.js";
  import { browseFilter } from "#lib/browse/filter.js";
  import ExpandAll from "#lib/components/files/ExpandAll.svelte";
  import FileCells from "#lib/components/files/FileCells.svelte";
  import FileHeader from "#lib/components/files/FileHeader.svelte";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import FolderLine from "#lib/components/files/FolderLine.svelte";
  import RemoteFileItems from "#lib/components/files/RemoteFileItems.svelte";
  import RemoteFolderItems from "#lib/components/files/RemoteFolderItems.svelte";
  import SelectionBar from "#lib/components/files/SelectionBar.svelte";
  import InspectorToggle from "#lib/components/inspector/InspectorToggle.svelte";
  import RemoteFileInspector from "#lib/components/inspector/RemoteFileInspector.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Progress from "#lib/components/ui/Progress.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { fileColumns, fileTemplate, fileWidth } from "#lib/files/columns.js";
  import {
    asItem,
    downloadPlan,
    type Enqueued,
    enqueue,
    enqueueJob,
    followDownloads
  } from "#lib/files/downloads.js";
  import { FILTER_PLACEHOLDER } from "#lib/files/filters.js";
  import { formatBytes, getFileName, plural } from "#lib/format.js";
  import { inspector, useInspector } from "#lib/inspector/inspector.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { sharesHref, userHref } from "#lib/nav.js";
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
  import { TYPING_PAUSE } from "#lib/ui/keys.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  type Props = {
    /** whose shares these are */
    user: string;
    /** a folder of theirs to go to once the browse is here */
    path: string | null;
  };

  let { user, path }: Props = $props();

  /** how often the progress is asked for while the stream can't push it */
  const POLL_MS = 500;

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

  /** the file the inspector shows, which follows focus in the tree, and whose share it is in */
  let picked = $state.raw<{ username: string; file: RemoteFile } | null>(null);
  /** the picked file while the filter leaves it in this user's tree */
  const shown = $derived(
    picked?.username === browser.username && (!filter || filter.item(picked.file)) ? picked : null
  );
  useInspector(
    inspected,
    () => !!shown,
    () => tree?.focus()
  );
  const inspect = (file: RemoteFile) => (picked = { username: browser.username, file });

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

  // the user's browse is what was left in this browser, what is shown, or asked for now; then
  // the tree goes to the folder the address names, unless the address moved on meanwhile
  $effect(() => {
    const folder = path;
    const wanted = user;
    let current = true;
    untrack(async () => {
      await browser.restore(wanted);
      if (!current) return;
      if (browser.username !== wanted || browser.phase !== "complete") {
        await browser.browse(wanted);
      }
      if (current && folder) browser.target = folder;
    });
    return () => (current = false);
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
    const timer = setTimeout(() => (query = text), TYPING_PAUSE);
    return () => clearTimeout(timer);
  });

  // the open folders and the ticked files are kept a moment after they change
  $effect(() => {
    void expansion.snapshot();
    void [...browser.selected];
    const timer = setTimeout(browser.save, 300);
    return () => clearTimeout(timer);
  });

  const info = $derived(browser.info);

  // the user, the tab, then each folder down to the line with focus; a folder step goes to its
  // line
  useCrumbs(() => {
    const steps = [
      { label: user, href: userHref(user) },
      { label: "Shares", href: sharesHref(user) }
    ];
    const line = browser.username === user ? tree?.focused() : null;
    if (!line) return steps;
    const at = line.kind === "folder" ? line.node.path : line.folder.path;
    const sep = browser.separator;
    const parts = at.split(sep);
    return [
      ...steps,
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
  const ticked = $derived(tickedCounts(browser.selected));

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
   * Downloads files as one job: each top folder's as groups that keep their folders below the
   * deepest one holding them
   */
  const download = (list: RemoteFile[]): Promise<Enqueued> =>
    enqueueJob(
      downloadPlan(list, topOf, browser.separator).map((group) => ({
        username: browser.username,
        files: group.files.map((f) => asItem(f)),
        destination: group.destination ?? undefined
      }))
    );

  /** Every file under a folder, filtered or not, as one job in a folder named like it */
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

  const details = (file: RemoteFile) => {
    inspect(file);
    inspector.show();
  };
</script>

{#snippet folderItems(node: FolderNode<RemoteFile>)}
  <RemoteFolderItems
    folder={node.path}
    username={browser.username}
    ondownload={write && node.unlocked ? () => void downloadFolder(node) : undefined}
    browse={false} />
{/snippet}

{#snippet inspected()}
  {#if shown}
    {@const file = shown.file}
    <RemoteFileInspector
      {file}
      username={shown.username}
      ondownload={write && !file.locked ? () => void download([file]) : undefined} />
  {/if}
{/snippet}

<div class="flex flex-col gap-section" role="region" aria-label="Shares">
  {#if browser.phase === "pending"}
    {@const percent = Math.round(browser.progress?.percentComplete ?? 0)}
    <div class="flex flex-col items-center gap-3 py-10" role="status">
      <p class="text-lg">Loading shares... {percent}%</p>
      <Progress value={percent} label="Browse progress" class="w-64 ring-1 ring-border" />
      <div class="mt-4 flex w-full max-w-xl flex-col gap-2" aria-hidden="true">
        {#each [0, 1, 2, 3, 4] as n (n)}
          <Skeleton class={`h-5 ${["", "ml-6", "ml-12"][n % 3]}`} />
        {/each}
      </div>
    </div>
  {:else if browser.phase === "error"}
    <div class="mt-12 flex flex-col items-center gap-1 text-center">
      <p class="text-lg">Could not browse their shares</p>
      {#if browser.errorMessage}<p class="text-sm text-fg-muted">{browser.errorMessage}</p>{/if}
      <Button variant="outline" size="sm" class="mt-3" onclick={() => void browser.browse(user)}>
        <RefreshCwIcon />Try again
      </Button>
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
    <section class="flex flex-col rounded-card border bg-surface">
      <header class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 pt-3">
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
        <Button variant="ghost" size="sm" class="ml-auto" onclick={() => void browser.browse(user)}>
          <RefreshCwIcon />Browse again
        </Button>
      </header>
      <div class="flex flex-wrap items-center gap-inline px-4 py-3">
        <div class="min-w-48 flex-1">
          <Input
            bind:value={typed}
            type="search"
            aria-label="Filter"
            placeholder={FILTER_PLACEHOLDER} />
        </div>
        <ExpandAll {expansion} />
        <InspectorToggle />
      </div>
    </section>
    <FileTree
      bind:this={tree}
      {lines}
      {expansion}
      place={`browse.${browser.username}`}
      label="Shared folders"
      columns={fileTemplate(columns)}
      width={fileWidth(columns)}
      height="max(20rem, calc(var(--content-h) - 16rem))"
      selected={browser.selected}
      idOf={(f) => f.filename}
      tickable={(f) => !f.locked}
      nameOf={(line) =>
        line.kind === "folder" ? line.node.label : getFileName(line.item.filename)}
      onopen={write ? (f) => void download([f]) : undefined}
      onmiddleclick={write ? (f) => void download([f]) : undefined}
      {inspect}>
      {#snippet header(all)}
        <FileHeader {columns} {all} />
      {/snippet}
      {#snippet menu(line)}
        {#if line.kind === "folder"}
          {@render folderItems(line.node)}
        {:else}
          <RemoteFileItems
            path={line.item.filename}
            username={browser.username}
            ondownload={write && !line.item.locked ? () => void download([line.item]) : undefined}
            ondetails={() => details(line.item)} />
        {/if}
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
            {@render folderItems(node)}
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
          ondownload={write && !file.locked ? () => void download([file]) : undefined}
          ondetails={() => details(file)} />
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
