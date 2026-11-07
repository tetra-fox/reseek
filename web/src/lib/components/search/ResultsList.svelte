<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import type { SvelteSet } from "svelte/reactivity";

  import FileCells from "#lib/components/files/FileCells.svelte";
  import FileMenu from "#lib/components/files/FileMenu.svelte";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import RemoteFileItems from "#lib/components/files/RemoteFileItems.svelte";
  import RemoteFolderItems from "#lib/components/files/RemoteFolderItems.svelte";
  import SelectionBar from "#lib/components/files/SelectionBar.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Flag from "#lib/components/ui/Flag.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import type { AllFiles } from "#lib/files/allfiles.svelte.js";
  import type { FileColumn } from "#lib/files/columns.js";
  import { asItem, type Enqueued, enqueueJob, followDownloads } from "#lib/files/downloads.js";
  import { formatBytes, formatSpeed, getFileName, plural } from "#lib/format.js";
  import { inspector } from "#lib/inspector/inspector.svelte.js";
  import { heldOrder } from "#lib/lists/interaction.svelte.js";
  import { Grid } from "#lib/lists/table.svelte.js";
  import { offerFields, type Picked } from "#lib/search/facts.js";
  import {
    RESULT_COLUMNS,
    type ResultColumn,
    resultList,
    type ResultRow
  } from "#lib/search/list.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { NO_FOLDER, type TreeLine } from "#lib/tree/folders.js";
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
  };

  let { rows, searchId, canDownload, inspect, ondismiss, allFiles, selected }: Props = $props();

  /** the file's own cells, drawn as every list of remote files draws them */
  const FILE_CELLS: FileColumn[] = ["name", "size", "quality", "length"];
  /** a line's height in rem, the same on a phone, which scrolls the columns sideways */
  const LINE_REM = { wide: 1.75, phone: 1.75 };

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
    clearable: true
  });

  // while the user holds the list still, rows keep their places and new ones go below
  const hold = heldOrder<ResultRow>((r) => r.key);
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

  /** the files of each user's folder, for a row's "Download this folder" */
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

  /** Enqueues rows as one job, one group per user, and says what came of it */
  const download = (list: ResultRow[]): Promise<Enqueued> => {
    const byUser = new Map<string, ResultRow[]>();
    for (const r of list) {
      const mine = byUser.get(r.username);
      if (mine) mine.push(r);
      else byUser.set(r.username, [r]);
    }
    return enqueueJob(
      [...byUser].map(([username, mine]) => ({
        username,
        files: mine.map((r) => asItem(r.file)),
        searchId
      }))
    );
  };
  /** Downloads rows and says what came of it in a toast */
  const downloadNow = async (list: ResultRow[]) => {
    const outcome = await download(list);
    toast(outcome.text, outcome.kind === "done" ? "info" : "error");
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

<FileTree
  bind:this={tree}
  {lines}
  {expansion}
  label="Search results"
  columns={grid.template}
  width={grid.width}
  height="max(20rem, calc(var(--content-h) - 16rem))"
  {selected}
  idOf={(r) => r.key}
  nameOf={(line) => (line.kind === "item" ? getFileName(line.item.file.filename) : "")}
  lineRem={LINE_REM}
  framed={false}
  class="tabular-nums"
  onopen={canDownload ? (r) => void downloadNow(selected.size > 1 ? chosen : [r]) : undefined}
  onmiddleclick={canDownload ? (r) => void downloadNow([r]) : undefined}
  inspect={(r) => inspect(pick(r))}>
  {#snippet header(all)}
    <GridHeader
      {grid}
      {all}
      class="h-8 border-b bg-surface-muted text-sm font-medium text-fg-muted" />
  {/snippet}
  {#snippet menu(line)}
    {#if line.kind === "item"}
      {@const row = line.item}
      {@const folder = folderOf(row)}
      <RemoteFileItems
        path={row.file.filename}
        username={row.username}
        ondownload={canDownload ? () => void downloadNow([row]) : undefined}
        ondetails={() => details(row)} />
      <Menu.Separator />
      <!-- the whole path, which a menu has no room to cut and show on hover -->
      <Menu.Label class="wrap-anywhere">{row.folder}</Menu.Label>
      <RemoteFolderItems
        folder={row.folder}
        username={row.username}
        ondownload={canDownload ? () => void downloadNow(folder) : undefined}
        onallfiles={() => void allFiles.show(row.username, row.folder)}
        browse={false} />
      <Menu.Separator />
      <ResultUserItems
        name={row.username}
        named
        folder={row.folder}
        ondownload={canDownload
          ? () => void downloadNow(rows.filter((r) => r.username === row.username))
          : undefined}
        ondismiss={() => ondismiss(row.username)} />
    {/if}
  {/snippet}
  {#snippet folder()}{/snippet}
  {#snippet item(row, _depth, tick)}
    {@const fileName = getFileName(row.file.filename)}
    {@const folder = folderOf(row)}
    <span role="gridcell" class="flex px-2">
      <Checkbox
        aria-label={`Select ${fileName}`}
        checked={selected.has(row.key)}
        onclick={(e) => {
          e.preventDefault();
          tick(e.shiftKey);
        }}
        onCheckedChange={(on) => on !== selected.has(row.key) && tick(false)} />
    </span>
    <span role="gridcell" class="flex min-w-0 items-center gap-1.5 px-2">
      <span class="min-w-0 truncate"><Username name={row.username} folder={row.folder} /></span>
      <Flag code={row.country} />
    </span>
    <span role="gridcell" class="truncate px-2 text-right tabular-nums"
      >{formatSpeed(row.speed)}</span>
    <span role="gridcell" class="truncate px-2 text-right tabular-nums">{row.queue}</span>
    <span role="gridcell" class={["px-2", row.free ? "text-success" : "text-fg-muted"]}>
      {row.free ? "Yes" : "No"}
    </span>
    <FileCells
      file={row.file}
      username={row.username}
      columns={FILE_CELLS}
      selected={selected.has(row.key)}
      ontick={tick} />
    <Tooltip text={row.folder} clipped>
      {#snippet child({ props })}
        <span {...props} role="gridcell" class="truncate px-2 font-mono text-xs text-fg-muted">
          {row.folder}
        </span>
      {/snippet}
    </Tooltip>
    <span role="gridcell" class="flex items-center justify-end gap-0.5 pr-1 row-action">
      {#if canDownload}
        <Tooltip text={`${A.downloadFolder.label}: ${folderText(folder)}`}>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label={`${A.downloadFolder.label}, ${folderText(folder)}`}
              onclick={() => void downloadNow(folder)}>
              <A.downloadFolder.icon />
            </Button>
          {/snippet}
        </Tooltip>
      {/if}
      <FileMenu
        path={row.file.filename}
        username={row.username}
        ondownload={canDownload ? () => void downloadNow([row]) : undefined}
        ondetails={() => details(row)}>
        {#snippet trigger({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label={`Actions for ${fileName}`}>
            <EllipsisIcon />
          </Button>
        {/snippet}
      </FileMenu>
    </span>
  {/snippet}
</FileTree>
{#if chosen.length}
  <SelectionBar
    files={chosen.map((r) => r.file)}
    ondownload={canDownload ? () => download(chosen) : undefined}
    onclear={() => selected.clear()} />
{/if}
