<script lang="ts" module>
  import { gridView } from "#lib/table.svelte.js";

  /** the whole-folder list's sort and column widths, kept in this browser */
  const view = gridView("reseek.folder.files", ["path", "size", "attributes"], {
    id: "path",
    dir: "asc"
  });
</script>

<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import { SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import { asItem, enqueue } from "#lib/downloads.js";
  import { formatAttributes, formatBytes, plural } from "#lib/format.js";
  import { fileMatches, parseFilters } from "#lib/search/filters.js";
  import { separatorOf } from "#lib/search/results.js";
  import { session } from "#lib/session.svelte.js";
  import { Grid, keptIn } from "#lib/table.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { rem, viewport } from "#lib/viewport.svelte.js";
  import { ListWindow } from "#lib/virtual.svelte.js";

  import Suspect from "./Suspect.svelte";

  type Props = {
    open: boolean;
    username: string;
    /** the remote folder, whose path the files' paths start with */
    folder: string;
    /** every file under it, each `filename` the full remote path */
    files: Schemas["File"][];
    /** the app is connected to the Soulseek server, which a download needs */
    connected: boolean;
  };

  let { open = $bindable(), username, folder, files, connected }: Props = $props();

  let text = $state("");
  let busy = $state(false);
  let headerHeight = $state(0);
  const selected = new SvelteSet<string>();

  const prefix = $derived(folder + separatorOf(folder));
  const relative = (path: string) => (path.startsWith(prefix) ? path.slice(prefix.length) : path);
  const shown = $derived.by(() => {
    const filters = parseFilters(text);
    return files.filter((f) => fileMatches(filters, f));
  });
  type File = Schemas["File"];
  const grid = new Grid<File>({
    columns: () => [
      { id: "select", label: "Select", width: "2.5rem", quiet: true, fixed: true },
      // a phone gives the path whatever the size leaves, and has no room for the attributes
      {
        id: "path",
        label: "Path",
        width: viewport.phone ? "0rem" : "16rem",
        grow: true,
        value: (f) => relative(f.filename).toLowerCase()
      },
      { id: "size", label: "Size", width: "6rem", end: true, value: (f) => f.size },
      { id: "attributes", label: "Attributes", width: "9rem", value: (f) => f.bitRate ?? -1 }
    ],
    data: () => shown,
    shown: () => ["select", "path", "size", ...(viewport.phone ? [] : ["attributes"])],
    ...keptIn(view)
  });
  const rows = $derived(grid.rows);
  // every file is listed, the window drawing those in view; a long path wraps, so each row's
  // height is measured
  const lines = new ListWindow(
    () => rows,
    (f) => f.filename,
    () => ({
      estimateSize: () => 2 * rem.px,
      overscan: 8,
      scrollMargin: headerHeight,
      scrollPaddingStart: headerHeight,
      sizes: rem.px
    })
  );
  const ticked = $derived(rows.filter((f) => selected.has(f.filename)).length);
  /** the downloads land in a folder named like the remote one */
  const destination = $derived(folder.split(separatorOf(folder)).filter(Boolean).pop());

  const download = async (chosen: Schemas["File"][]) => {
    busy = true;
    const result = await enqueue({
      username,
      files: chosen.map((f) => asItem(f)),
      destination
    });
    busy = false;
    toast(result.text, result.kind === "done" ? "info" : "error");
  };
</script>

<Dialog
  bind:open
  title={folder}
  description={`Every file in this folder and its subfolders, from ${username}`}
  class="w-[min(56rem,calc(100vw-2rem))]">
  <Input bind:value={text} type="search" placeholder="Filter files" aria-label="Filter" />
  <div
    class="-mx-5 max-h-[min(55vh,32rem)] overflow-auto border-y text-sm"
    role="grid"
    aria-label="Files"
    aria-rowcount={rows.length}
    {@attach lines.attach}>
    <div class="sticky top-0 z-10" style:min-width={grid.width} bind:offsetHeight={headerHeight}>
      <GridHeader {grid} class="h-8 border-b bg-surface-muted font-medium text-fg-muted">
        {#snippet cell()}
          <Checkbox
            aria-label="Select every file listed"
            checked={rows.length > 0 && ticked === rows.length}
            indeterminate={ticked > 0 && ticked < rows.length}
            onCheckedChange={(on) => {
              for (const f of rows) {
                if (on) selected.add(f.filename);
                else selected.delete(f.filename);
              }
            }} />
        {/snippet}
      </GridHeader>
    </div>
    <div class="relative" style:height={`${lines.total}px`} style:min-width={grid.width}>
      {#each lines.drawn as { line: file, index, start } (file.filename)}
        <div
          role="row"
          class="absolute inset-x-0 top-0 grid items-center list-row py-1"
          style:transform={`translateY(${start - headerHeight}px)`}
          style:grid-template-columns={grid.template}
          data-index={index}
          data-last={index === rows.length - 1 || undefined}
          {@attach lines.measure}>
          <span role="gridcell" class="flex justify-center">
            <Checkbox
              aria-label={`Select ${relative(file.filename)}`}
              checked={selected.has(file.filename)}
              onCheckedChange={(on) =>
                on ? selected.add(file.filename) : selected.delete(file.filename)} />
          </span>
          <span role="gridcell" class="min-w-0 px-2 font-mono text-xs wrap-anywhere">
            {relative(file.filename)}
          </span>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums"
            >{formatBytes(file.size)}</span>
          {#if !viewport.phone}
            <span role="gridcell" class="flex min-w-0 items-center gap-1 px-2 text-fg-muted">
              {#if file.suspect}<Suspect suspect={file.suspect} />{/if}
              <span class="truncate">{formatAttributes(file)}</span>
            </span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
  {#snippet footer()}
    {#if session.can("ReadWrite")}
      <Button
        variant="outline"
        disabled={busy || !connected || !ticked}
        onclick={() => void download(rows.filter((f) => selected.has(f.filename)))}>
        <DownloadIcon />Download {plural(ticked, "file", "files")}
      </Button>
      <Button disabled={busy || !connected || !files.length} onclick={() => void download(files)}>
        <DownloadIcon />Download all
      </Button>
    {/if}
  {/snippet}
</Dialog>
