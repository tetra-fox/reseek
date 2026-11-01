<script lang="ts">
  import SearchIcon from "@lucide/svelte/icons/search";
  import { untrack } from "svelte";

  import type { Target } from "#lib/analysis/analysis.js";
  import { analyzable, marks } from "#lib/analysis/marks.svelte.js";
  import { api } from "#lib/api/client.js";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import FolderLine from "#lib/components/files/FolderLine.svelte";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { formatBytes, getFileName } from "#lib/format.js";
  import { call } from "#lib/request.js";
  import type { Root } from "#lib/system/localfiles.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { folderNode, folderTree, treeLines } from "#lib/tree/folders.js";

  import AnalysisMark from "./AnalysisMark.svelte";

  // the daemon's audio files as a tree with a filter box, to pick one for a slot: the shared tree
  // with no selection, actions or menus
  type Props = {
    onpick: (target: Target, name: string) => void;
    /** the tree's height, a CSS length */
    height: string;
  };

  let { onpick, height }: Props = $props();

  type Pick = { path: string; size: number };

  const ROOTS = [
    { id: "downloads", label: "Downloads" },
    { id: "incomplete", label: "Incomplete" }
  ] as const;

  let root = $state<Root>("downloads");
  let text = $state("");
  let picks = $state.raw<Pick[] | null>(null);
  let error = $state("");
  const expansion = new Expansion(false);

  $effect(() => marks.retain());
  /** Reads every file under the root; the listing's names are relative to it */
  const read = async (at: Root) => {
    picks = null;
    const outcome = await call(() =>
      api.GET("/api/v0/files/{root}/directories", {
        params: { path: { root: at }, query: { recursive: true } }
      })
    );
    if (at !== root) return;
    error = outcome.ok ? "" : outcome.error;
    if (!outcome.ok) return;
    picks = (outcome.data.files ?? [])
      .filter((f) => analyzable(f.name))
      .map((f) => ({ path: f.fullName.replaceAll("\\", "/"), size: f.length }))
      .toSorted((a, b) => a.path.localeCompare(b.path, undefined, { numeric: true }));
  };
  $effect(() => {
    const at = root;
    untrack(() => {
      expansion.reset(false);
      void read(at);
    });
  });

  const sizeOf = (f: Pick) => f.size;
  const keyOf = (f: Pick) => f.path;
  const words = $derived(text.toLowerCase().split(/\s+/).filter(Boolean));
  const matches = (path: string) => {
    const lower = path.toLowerCase();
    return words.every((word) => lower.includes(word));
  };
  /** files directly under the root, which have no folder to go in */
  const loose = $derived((picks ?? []).filter((f) => !f.path.includes("/")));
  const here = $derived(folderNode({ key: "", path: "", label: root }, loose, [], sizeOf));
  const nested = $derived(
    folderTree(
      (picks ?? []).filter((f) => f.path.includes("/")),
      (f) => f.path,
      sizeOf
    )
  );
  const lines = $derived([
    ...treeLines(
      nested,
      expansion.isOpen,
      keyOf,
      words.length ? { folder: (node) => matches(node.path), item: (f) => matches(f.path) } : null
    ),
    ...loose
      .filter((f) => matches(f.path))
      .map((item) => ({ kind: "item" as const, key: item.path, item, depth: 0, folder: here }))
  ]);

  const pick = (f: Pick) => onpick({ kind: "file", root, path: f.path }, getFileName(f.path));
</script>

<div class="flex flex-col gap-2">
  <div class="flex flex-wrap items-center gap-inline">
    <div class="min-w-40 flex-1">
      <Input
        bind:value={text}
        aria-label="Filter files"
        placeholder="Filter files"
        icon={SearchIcon} />
    </div>
    <Segmented options={ROOTS} bind:value={root} label="Folder" size="sm" />
  </div>
  {#if error}
    <Notice kind="error">{error}</Notice>
  {:else if !picks}
    <Skeleton class="h-40" />
  {:else if !lines.length}
    <EmptyRow text={text ? "No files match the filter" : "No audio files here"} />
  {:else}
    <FileTree
      {lines}
      {expansion}
      {height}
      label={`Audio files in ${root}`}
      columns="minmax(0,1fr)"
      idOf={keyOf}
      onopen={pick}
      nameOf={(line) => (line.kind === "folder" ? line.node.label : getFileName(line.item.path))}>
      {#snippet folder(node, depth)}
        <FolderLine {node} open={expansion.isOpen(node.key)} {depth} />
      {/snippet}
      {#snippet item(f)}
        <button
          type="button"
          role="gridcell"
          class="flex h-full min-w-0 items-center gap-2 pr-3 text-left"
          title={f.path}
          onclick={() => pick(f)}>
          <span class="min-w-0 flex-1 truncate">{getFileName(f.path)}</span>
          <AnalysisMark mark={marks.file(root, f.path)} />
          <span class="shrink-0 text-xs text-fg-muted tabular-nums">{formatBytes(f.size)}</span>
        </button>
      {/snippet}
    </FileTree>
  {/if}
</div>
