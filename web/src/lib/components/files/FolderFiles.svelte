<script lang="ts">
  import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import { SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import { asItem, enqueue } from "#lib/downloads.js";
  import { formatAttributes, formatBytes, plural } from "#lib/format.js";
  import { fileMatches, parseFilters } from "#lib/search/filters.js";
  import { separatorOf } from "#lib/search/results.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

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

  /** rows listed at most; a bigger folder says so */
  const CAP = 20_000;

  let text = $state("");
  let sort = $state<{ by: "path" | "size"; asc: boolean }>({ by: "path", asc: true });
  let busy = $state(false);
  const selected = new SvelteSet<string>();

  const prefix = $derived(folder + separatorOf(folder));
  const relative = (path: string) => (path.startsWith(prefix) ? path.slice(prefix.length) : path);
  const shown = $derived.by(() => {
    const filters = parseFilters(text);
    const kept = files.filter((f) => fileMatches(filters, f));
    const sign = sort.asc ? 1 : -1;
    kept.sort((a, b) =>
      sort.by === "size"
        ? sign * (a.size - b.size)
        : sign * relative(a.filename).localeCompare(relative(b.filename))
    );
    return kept;
  });
  const rows = $derived(shown.slice(0, CAP));
  const ticked = $derived(rows.filter((f) => selected.has(f.filename)).length);
  /** the downloads land in a folder named like the remote one */
  const destination = $derived(folder.split(separatorOf(folder)).filter(Boolean).pop());

  const sortBy = (by: "path" | "size") => (sort = { by, asc: sort.by === by ? !sort.asc : true });

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
  description={`Every file under it from ${username}`}
  class="w-[min(56rem,calc(100vw-2rem))]">
  <Input
    bind:value={text}
    type="search"
    placeholder="Filter, like the search results"
    aria-label="Filter" />
  {#if shown.length > CAP}
    <Notice kind="warning">
      {CAP.toLocaleString()} of {shown.length.toLocaleString()} files listed; filter to find the rest.
    </Notice>
  {/if}
  <div class="-mx-5 overflow-x-auto border-y">
    <table class="data-table w-full text-sm">
      <thead>
        <tr>
          <th class="w-10">
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
          </th>
          <th>
            <button
              type="button"
              class="inline-flex items-center gap-1"
              onclick={() => sortBy("path")}>
              Path <ArrowUpDownIcon class="size-3" />
            </button>
          </th>
          <th class="text-right">
            <button
              type="button"
              class="inline-flex items-center gap-1"
              onclick={() => sortBy("size")}>
              Size <ArrowUpDownIcon class="size-3" />
            </button>
          </th>
          <th>Attributes</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as file (file.filename)}
          <tr>
            <td>
              <Checkbox
                aria-label={`Select ${relative(file.filename)}`}
                checked={selected.has(file.filename)}
                onCheckedChange={(on) =>
                  on ? selected.add(file.filename) : selected.delete(file.filename)} />
            </td>
            <td class="max-w-0 min-w-64 font-mono text-xs wrap-anywhere whitespace-normal">
              {relative(file.filename)}
            </td>
            <td class="text-right tabular-nums">{formatBytes(file.size)}</td>
            <td class="text-fg-muted">
              <span class="inline-flex items-center gap-1">
                {#if file.suspect}<Suspect suspect={file.suspect} />{/if}
                {formatAttributes(file)}
              </span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  {#snippet footer()}
    {#if session.can("ReadWrite")}
      <Button
        variant="outline"
        disabled={busy || !connected || !ticked}
        onclick={() => void download(rows.filter((f) => selected.has(f.filename)))}>
        <DownloadIcon />Download selected ({plural(ticked, "file", "files")})
      </Button>
      <Button disabled={busy || !connected || !files.length} onclick={() => void download(files)}>
        <DownloadIcon />Download all
      </Button>
    {/if}
  {/snippet}
</Dialog>
