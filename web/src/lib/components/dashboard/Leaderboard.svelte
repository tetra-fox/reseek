<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { untrack } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { links } from "#lib/components/users/actions.js";
  import Username from "#lib/components/users/Username.svelte";
  import { type RangeLabel, reports, type Window } from "#lib/dashboard/reports.js";
  import { formatBytes, formatSpeed } from "#lib/format.js";

  // who moved the most with us each way, by count, bytes or speed
  let { span, range }: { span: Window; range: RangeLabel } = $props();

  type Entry = Schemas["LeaderboardEntry"];
  type Sort = Schemas["LeaderboardSort"];

  /** the column last clicked, for the range it was clicked in */
  let picked = $state<{ range: RangeLabel; by: Sort } | null>(null);
  // a new range starts over at Count; a background reload of the same range keeps the column
  const sortBy = $derived(picked?.range === range ? picked.by : "Count");
  let rows = $state.raw<{ Download: Entry[]; Upload: Entry[] }>({ Download: [], Upload: [] });
  let loading = $state(true);
  let error = $state("");

  // a newer sort or range makes an older answer moot
  let asked = 0;
  $effect(() => {
    const w = span;
    const by = sortBy;
    const at = ++asked;
    untrack(async () => {
      loading = true;
      const [down, up] = await Promise.all([
        reports.leaderboard(w, "Download", by),
        reports.leaderboard(w, "Upload", by)
      ]);
      if (at !== asked) return;
      loading = false;
      // a failed load keeps the rows shown before
      if (!down.ok) error = down.error;
      else if (!up.ok) error = up.error;
      else {
        rows = { Download: down.data, Upload: up.data };
        error = "";
      }
    });
  });

  // short headers and fixed widths keep the figures in view on a phone; the name gives way
  const COLUMNS: { by: Sort; label: string; title: string; width: string }[] = [
    { by: "Count", label: "Files", title: "Files", width: "w-14" },
    { by: "TotalBytes", label: "Size", title: "Total size", width: "w-18" },
    { by: "AverageSpeed", label: "Speed", title: "Average speed", width: "w-22" }
  ];
</script>

{#if error}<p class="text-sm text-danger">{error}</p>{/if}
<div class="grid grid-cols-[minmax(0,1fr)] gap-section lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
  {#each [{ direction: "Download" as const, title: "Downloads", Icon: DownloadIcon }, { direction: "Upload" as const, title: "Uploads", Icon: UploadIcon }] as table (table.direction)}
    <section class="flex flex-col gap-2">
      <h3 class="flex items-center gap-1.5 text-sm font-semibold">
        <table.Icon class="size-4" />{table.title}
        {#if loading}<Spinner class="size-3.5" />{/if}
      </h3>
      <table class="w-full table-fixed text-sm">
        <thead class="text-xs text-fg-muted">
          <tr class="border-b">
            <th class="w-6 py-1 text-right font-medium">#</th>
            <th class="px-2 py-1 text-left font-medium">Username</th>
            {#each COLUMNS as c (c.by)}
              <th
                class={["px-2 py-1 text-right font-medium", c.width]}
                aria-sort={sortBy === c.by ? "descending" : "none"}>
                <button
                  type="button"
                  class="inline-flex items-center gap-1"
                  title={c.title}
                  onclick={() => (picked = { range, by: c.by })}>
                  {c.label}
                  {#if sortBy === c.by}<ArrowDownIcon class="size-3" />{/if}
                </button>
              </th>
            {/each}
            <!-- a phone leaves the browse button out, for the name's sake; the name's menu has it -->
            <th class="w-10 max-sm:hidden"><span class="sr-only">Browse</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows[table.direction] as entry, i (entry.username)}
            <tr class="border-b last:border-b-0">
              <td class="py-1 text-right text-fg-muted tabular-nums">{i + 1}</td>
              <td class="overflow-hidden px-2 py-1"><Username name={entry.username} /></td>
              <td class="px-2 py-1 text-right tabular-nums">{entry.count.toLocaleString()}</td>
              <td class="px-2 py-1 text-right whitespace-nowrap tabular-nums"
                >{formatBytes(entry.totalBytes)}</td>
              <td class="px-2 py-1 text-right whitespace-nowrap tabular-nums"
                >{formatSpeed(entry.averageSpeed)}</td>
              <td class="max-sm:hidden">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  href={links(entry.username).browse}
                  aria-label={`Browse ${entry.username}`}><FolderOpenIcon /></Button>
              </td>
            </tr>
          {:else}
            <tr
              ><td colspan="6" class="py-6 text-center text-fg-subtle"
                >No {table.title.toLowerCase()} in this range.</td
              ></tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/each}
</div>
