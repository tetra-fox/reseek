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
  import { columnWidth, formatBytes, formatSpeed, WIDEST } from "#lib/format.js";

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

  // each figure's column holds the widest value its formatter writes, and the name gives way;
  // a phone has no room for the speed as well, so it leaves that column out
  const COLUMNS: {
    by: Sort;
    label: string;
    title: string;
    widest: string;
    value: (e: Entry) => string;
    phone?: false;
  }[] = [
    {
      by: "Count",
      label: "Files",
      title: "Files",
      widest: WIDEST.count,
      value: (e) => e.count.toLocaleString()
    },
    {
      by: "TotalBytes",
      label: "Size",
      title: "Total size",
      widest: WIDEST.size,
      value: (e) => formatBytes(e.totalBytes)
    },
    {
      by: "AverageSpeed",
      label: "Speed",
      title: "Average speed",
      widest: WIDEST.speed,
      value: (e) => formatSpeed(e.averageSpeed),
      phone: false
    }
  ];
</script>

{#if error}<p class="text-sm text-danger">{error}</p>{/if}
<!-- the two tables sit side by side only where each gets 28rem, room for its figures and a
     name; in rem, so a larger text size needs a wider screen before they do -->
<div class="grid grid-cols-[repeat(auto-fit,minmax(min(100%,28rem),1fr))] gap-section">
  {#each [{ direction: "Download" as const, title: "Downloads", Icon: DownloadIcon }, { direction: "Upload" as const, title: "Uploads", Icon: UploadIcon }] as table (table.direction)}
    <section class="flex flex-col gap-2">
      <h3 class="flex items-center gap-1.5 text-sm font-semibold">
        <table.Icon class="size-4" />{table.title}
        {#if loading}<Spinner class="size-3.5" />{/if}
      </h3>
      <table class="w-full table-fixed text-sm tabular-nums">
        <!-- the widths are set here, where the font is the cells', so a width in ch is theirs and
             not the smaller header's; a phone leaves out the speed and the browse button, the
             name's menu having Browse, so the name keeps room -->
        <colgroup>
          <col class="w-6" />
          <col />
          {#each COLUMNS as c (c.by)}
            <col
              class={[c.phone === false && "max-sm:hidden"]}
              style:width={columnWidth(c.widest)} />
          {/each}
          <col class="w-10 max-sm:hidden" />
        </colgroup>
        <thead class="text-xs text-fg-muted">
          <tr class="border-b">
            <th class="py-1 text-right font-medium">#</th>
            <th class="px-2 py-1 text-left font-medium">Username</th>
            {#each COLUMNS as c (c.by)}
              <th
                class={["px-2 py-1 text-right font-medium", c.phone === false && "max-sm:hidden"]}
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
            <th class="max-sm:hidden"><span class="sr-only">Browse</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows[table.direction] as entry, i (entry.username)}
            <tr class="border-b last:border-b-0">
              <td class="py-1 text-right text-fg-muted tabular-nums">{i + 1}</td>
              <td class="overflow-hidden px-2 py-1"><Username name={entry.username} /></td>
              {#each COLUMNS as c (c.by)}
                <td
                  class={[
                    "px-2 py-1 text-right whitespace-nowrap tabular-nums",
                    c.phone === false && "max-sm:hidden"
                  ]}>{c.value(entry)}</td>
              {/each}
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
