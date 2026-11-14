<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import type { SvelteSet } from "svelte/reactivity";

  import FileCells from "#lib/components/files/FileCells.svelte";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import RemoteFileItems from "#lib/components/files/RemoteFileItems.svelte";
  import RemoteFolderItems from "#lib/components/files/RemoteFolderItems.svelte";
  import SelectionBar from "#lib/components/files/SelectionBar.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Flag from "#lib/components/ui/Flag.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import type { AllFiles } from "#lib/files/allfiles.svelte.js";
  import type { FileColumn } from "#lib/files/columns.js";
  import {
    downloadFolderPlan,
    type Enqueued,
    enqueuePlan,
    enqueueSelection,
    followDownloads
  } from "#lib/files/downloads.js";
  import { badgeCount, formatBytes, formatSpeed, getFileName, plural } from "#lib/format.js";
  import { inspector } from "#lib/inspector/inspector.svelte.js";
  import { type Freeze, heldOrder } from "#lib/lists/interaction.svelte.js";
  import { Grid } from "#lib/lists/table.svelte.js";
  import { offerFields, type Picked } from "#lib/search/facts.js";
  import {
    QUEUE_MOST,
    RESULT_COLUMNS,
    type ResultColumn,
    resultList,
    type ResultRow
  } from "#lib/search/list.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { NO_FOLDER, separatorOf, type TreeLine } from "#lib/tree/folders.js";
  import { users } from "#lib/users/users.svelte.js";

  import ResultUserItems from "./ResultUserItems.svelte";

  type Props = {
    /** one row per file, in the order the results are in */
    rows: ResultRow[];
    searchId: string;
    /** downloads need the Soulseek server and write access */
    canDownload: boolean;
    /** focus came to a file, which the inspector shows */
    inspect: (file: Picked) => void;
    ondismiss: (username: string) => void;
    /** the dialog that lists every file under a folder */
    allFiles: AllFiles;
    /** the ticked files, by their row's key, which the tree layout shares */
    selected: SvelteSet<string>;
    /** what holds the results still */
    freeze: Freeze;
  };

  let { rows, searchId, canDownload, inspect, ondismiss, allFiles, selected, freeze }: Props =
    $props();

  followDownloads();
  /** a list has no folders to open, the tree it is drawn by wants one */
  const expansion = new Expansion(true);
  let tree = $state<ReturnType<typeof FileTree<ResultRow>> | null>(null);

  const grid = new Grid<ResultRow>({
    columns: () => RESULT_COLUMNS,
    data: () => rows,
    sort: () => resultList.sort,
    setSort: (sort) => (resultList.sort = sort as typeof resultList.sort),
    dragged: () => resultList.dragged,
    drag: (id, width) => (resultList.dragged[id as ResultColumn] = width),
    shown: () =>
      RESULT_COLUMNS.filter((c) => !resultList.hidden.includes(c.id as ResultColumn)).map(
        (c) => c.id
      ),
    clearable: true
  });
  /** the columns shown, which the row's cells follow */
  const shows = $derived(new Set(grid.columns.map((c) => c.id)));
  /** the file's own cells, drawn as every list of remote files draws them */
  const fileCells = $derived<FileColumn[]>([
    "name",
    "size",
    ...(shows.has("bitrate") ? (["quality"] as const) : []),
    ...(shows.has("length") ? (["length"] as const) : [])
  ]);

  // while the user holds the list still, rows keep their places and new ones go below
  // svelte-ignore state_referenced_locally
  const hold = heldOrder<ResultRow>((r) => r.key, freeze);
  const sorted = $derived(hold(grid.rows));
  const lines = $derived(
    sorted.map((r): TreeLine<ResultRow> => ({
      kind: "item",
      key: r.key,
      item: r,
      depth: 0,
      folder: NO_FOLDER
    }))
  );
  const chosen = $derived(sorted.filter((r) => selected.has(r.key)));

  /** the files of each user's folder, for a row's "Download folder" */
  const folders = $derived.by(() => {
    const map = new Map<string, ResultRow[]>();
    for (const r of rows) {
      const key = `${r.username}\u0000${r.folder}`;
      const list = map.get(key);
      if (list) list.push(r);
      else map.set(key, [r]);
    }
    return map;
  });
  const folderOf = (r: ResultRow) => folders.get(`${r.username}\u0000${r.folder}`) ?? [r];
  const folderText = (folder: ResultRow[]) =>
    `${plural(folder.length, "file", "files")}, ${formatBytes(folder.reduce((n, r) => n + r.file.size, 0))}`;

  // the users on screen are looked up, for their presence and the offline toggle
  $effect(() => {
    const drawn = tree?.drawn() ?? [];
    for (const name of new Set(
      drawn.flatMap((l) => (l.kind === "item" ? [l.item.username] : []))
    )) {
      void users.load(name);
    }
  });

  /** Enqueues rows as one job that keeps each user's folders */
  const download = (list: ResultRow[]): Promise<Enqueued> =>
    enqueueSelection(
      list.map((r) => ({ username: r.username, file: r.file })),
      searchId
    );
  const tell = (outcome: Enqueued) =>
    toast(outcome.text, outcome.kind === "done" ? "info" : "error");
  /** Downloads rows and says what came of it in a toast */
  const downloadNow = async (list: ResultRow[]) => tell(await download(list));
  /** Downloads the files of a row's folder in the folder, so it keeps its own level */
  const downloadFolder = async (row: ResultRow) => {
    const plan = downloadFolderPlan(
      row.folder,
      folderOf(row).map((r) => r.file),
      separatorOf(row.folder)
    );
    tell(await enqueuePlan(row.username, plan, searchId));
  };

  /** A row's file for the inspector, with what its user offers */
  const pick = (r: ResultRow): Picked => ({
    file: r.file,
    username: r.username,
    facts: offerFields(r.speed, r.free, r.queue)
  });
  const details = (r: ResultRow) => {
    inspect(pick(r));
    inspector.show();
  };

  /** Gives the list DOM focus, without scrolling */
  export const focus = () => tree?.focus();
</script>

<!-- a file row's whole menu: the file, its folder and its user, which the list has no lines for.
     A bare file name has no folder -->
{#snippet rowItems(row: ResultRow)}
  {@const folder = folderOf(row)}
  <RemoteFileItems
    path={row.file.filename}
    username={row.username}
    ondownload={canDownload ? () => void downloadNow([row]) : undefined}
    ondetails={() => details(row)} />
  <Menu.Separator />
  {#if row.folder}
    <!-- the whole path, which a menu has no room to cut and show on hover -->
    <Menu.Label class="wrap-anywhere">{row.folder}</Menu.Label>
    <RemoteFolderItems
      folder={row.folder}
      username={row.username}
      ondownload={canDownload ? () => void downloadFolder(row) : undefined}
      onallfiles={() => void allFiles.show(row.username, row.folder)}
      browse={false} />
    <Menu.Separator />
  {/if}
  <ResultUserItems
    name={row.username}
    named
    folder={row.folder}
    ondownload={canDownload
      ? () => void downloadNow(rows.filter((r) => r.username === row.username))
      : undefined}
    ondismiss={() => ondismiss(row.username)} />
{/snippet}

<FileTree
  bind:this={tree}
  {lines}
  {expansion}
  {freeze}
  label="Search results"
  place={`search.${searchId}.list`}
  columns={grid.template}
  width={grid.width}
  height="max(20rem, calc(var(--content-h) - 16rem))"
  {selected}
  idOf={(r) => r.key}
  nameOf={(line) => (line.kind === "item" ? getFileName(line.item.file.filename) : "")}
  oneLine
  framed={false}
  class="tabular-nums"
  onopen={canDownload ? (r) => void downloadNow(selected.size > 1 ? chosen : [r]) : undefined}
  onmiddleclick={canDownload ? (r) => void downloadNow([r]) : undefined}
  inspect={(r) => inspect(pick(r))}>
  {#snippet header(all)}
    <GridHeader
      {grid}
      {all}
      rowindex={1}
      class="h-8 border-b bg-surface-muted text-sm text-fg-muted" />
  {/snippet}
  <!-- a right-click, a long press or the menu key opens what the row's ... button offers -->
  {#snippet menu(line)}
    {#if line.kind === "item"}{@render rowItems(line.item)}{/if}
  {/snippet}
  {#snippet folder()}{/snippet}
  {#snippet item(row, _depth, tick)}
    {@const fileName = getFileName(row.file.filename)}
    {@const folder = folderOf(row)}
    <span role="gridcell" class="flex px-2">
      <Checkbox aria-label={`Select ${fileName}`} checked={selected.has(row.key)} ontick={tick} />
    </span>
    <span role="gridcell" class="flex min-w-0 items-center gap-1.5 px-2">
      <span class="min-w-0 truncate"><Username name={row.username} folder={row.folder} /></span>
      <Flag code={row.country} />
    </span>
    {#if shows.has("speed")}
      <span role="gridcell" class="truncate px-2 text-right tabular-nums"
        >{formatSpeed(row.speed)}</span>
    {/if}
    {#if shows.has("queue")}
      <span role="gridcell" class="truncate px-2 text-right tabular-nums"
        >{badgeCount(row.queue, QUEUE_MOST)}</span>
    {/if}
    {#if shows.has("free")}
      <span role="gridcell" class={["px-2", row.free ? "text-success-soft-fg" : "text-fg-muted"]}>
        {row.free ? "Yes" : "No"}
      </span>
    {/if}
    <FileCells
      file={row.file}
      username={row.username}
      columns={fileCells}
      selected={selected.has(row.key)}
      ontick={tick} />
    {#if shows.has("folder")}
      <Tooltip text={row.folder || undefined} dense>
        {#snippet child({ props })}
          <span
            {...props}
            role="gridcell"
            class="flex min-w-0 px-2 font-mono text-xs text-fg-muted">
            <MiddleTruncate text={row.folder} path />
          </span>
        {/snippet}
      </Tooltip>
    {/if}
    <span role="gridcell" class="flex items-center justify-end gap-0.5 pr-1 row-action">
      {#if canDownload && row.folder}
        <Tooltip text={`${A.downloadFolder.label}: ${folderText(folder)}`}>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label={`${A.downloadFolder.label}, ${folderText(folder)}`}
              onclick={() => void downloadFolder(row)}>
              <A.downloadFolder.icon />
            </Button>
          {/snippet}
        </Tooltip>
      {/if}
      <Menu.LazyRoot>
        {#snippet trigger({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label={`Actions for ${fileName}`}>
            <EllipsisIcon />
          </Button>
        {/snippet}
        <Menu.Content align="end" class="min-w-56">
          {@render rowItems(row)}
        </Menu.Content>
      </Menu.LazyRoot>
    </span>
  {/snippet}
</FileTree>
{#if chosen.length}
  <SelectionBar
    files={chosen.map((r) => r.file)}
    ondownload={canDownload ? () => download(chosen) : undefined}
    onclear={() => selected.clear()} />
{/if}
