<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { untrack } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { type RangeLabel, reports, type Window } from "#lib/dashboard/reports.js";
  import { columnWidth, formatBytes, formatSpeed, plural, WIDEST } from "#lib/format.js";
  import { links } from "#lib/users/actions.js";

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
  // a phone has no room for the speed as well, so it leaves that column out. `brief` is the
  // figure as it reads under the name, where a table too narrow for the columns puts them
  const COLUMNS: {
    by: Sort;
    label: string;
    title: string;
    widest: string;
    value: (e: Entry) => string;
    brief?: (e: Entry) => string;
    phone?: false;
  }[] = [
    {
      by: "Count",
      label: "Files",
      title: "Files",
      widest: WIDEST.count,
      value: (e) => e.count.toLocaleString(),
      brief: (e) => plural(e.count, "file", "files")
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
    <!-- below 19rem, about a name's room beside the figures, the figures go under the name and
         the header's sorting moves into a switch by the title; in rem so it follows the text -->
    <section class="@container flex flex-col gap-2">
      <div class="flex items-center gap-2">
        <h3 class="flex items-center gap-1.5 text-sm font-semibold">
          <table.Icon class="size-4" />{table.title}
          {#if loading}<Spinner class="size-3.5" />{/if}
        </h3>
        <div
          class="ml-auto inline-flex rounded-control border p-0.5 @min-[19rem]:hidden"
          role="radiogroup"
          aria-label={`Sort ${table.title.toLowerCase()} by`}>
          {#each COLUMNS.filter((c) => c.phone !== false) as c (c.by)}
            <button
              type="button"
              role="radio"
              aria-checked={sortBy === c.by}
              class={[
                "rounded-badge px-2 py-0.5 text-xs",
                sortBy === c.by ? "bg-surface-muted font-medium" : "text-fg-muted"
              ]}
              onclick={() => (picked = { range, by: c.by })}>{c.label}</button>
          {/each}
        </div>
      </div>
      <table class="w-full table-fixed text-sm tabular-nums">
        <!-- the widths are set here, where the font is the cells', so a width in ch is theirs and
             not the smaller header's; a phone leaves out the speed and the browse button, the
             name's menu having Browse, so the name keeps room -->
        <colgroup>
          <col class="w-6" />
          <col />
          {#each COLUMNS as c (c.by)}
            <col
              class={[c.phone === false && "max-md:hidden", "@max-[19rem]:hidden"]}
              style:width={columnWidth(c.widest)} />
          {/each}
          <col class="w-10 max-md:hidden" />
        </colgroup>
        <thead class="text-xs text-fg-muted">
          <tr class="border-b">
            <th class="py-1 text-right font-medium">#</th>
            <th class="truncate px-2 py-1 text-left font-medium">Username</th>
            {#each COLUMNS as c (c.by)}
              <th
                class={[
                  "overflow-hidden px-2 py-1 text-right font-medium @max-[19rem]:hidden",
                  c.phone === false && "max-md:hidden"
                ]}
                aria-sort={sortBy === c.by ? "descending" : "none"}>
                <button
                  type="button"
                  class="inline-flex max-w-full items-center gap-1"
                  title={c.title}
                  onclick={() => (picked = { range, by: c.by })}>
                  <span class="truncate">{c.label}</span>
                  {#if sortBy === c.by}<ArrowDownIcon class="size-3 shrink-0" />{/if}
                </button>
              </th>
            {/each}
            <th class="max-md:hidden"><span class="sr-only">Browse</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows[table.direction] as entry, i (entry.username)}
            <tr class="border-b last:border-b-0">
              <td class="py-1 text-right text-fg-muted tabular-nums">{i + 1}</td>
              <td class="overflow-hidden px-2 py-1">
                <Username name={entry.username} />
                <div class="truncate text-xs text-fg-muted @min-[19rem]:hidden">
                  {COLUMNS.filter((c) => c.phone !== false)
                    .map((c) => (c.brief ?? c.value)(entry))
                    .join(" · ")}
                </div>
              </td>
              {#each COLUMNS as c (c.by)}
                <td
                  class={[
                    "px-2 py-1 text-right whitespace-nowrap tabular-nums @max-[19rem]:hidden",
                    c.phone === false && "max-md:hidden"
                  ]}>{c.value(entry)}</td>
              {/each}
              <td class="py-0.5 max-md:hidden">
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
