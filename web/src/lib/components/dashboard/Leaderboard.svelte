<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { untrack } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { type RangeLabel, reports, type Window } from "#lib/dashboard/reports.js";
  import {
    columnWidth,
    formatBytes,
    formatCount,
    formatSpeed,
    plural,
    WIDEST
  } from "#lib/format.js";
  import { reach, USER_ACTIONS } from "#lib/users/actions.js";

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
    /** the whole name, where the label shortens it */
    title?: string;
    widest: string;
    value: (e: Entry) => string;
    brief?: (e: Entry) => string;
    phone?: false;
  }[] = [
    {
      by: "Count",
      label: "Files",
      widest: WIDEST.count,
      value: (e) => formatCount(e.count),
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
  /** the columns a phone keeps, for the switch that sorts where the header can't show */
  const SORTS = COLUMNS.filter((c) => c.phone !== false).map((c) => ({ id: c.by, label: c.label }));
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
        <h3 class="flex items-center gap-1.5 text-sm font-medium">
          <table.Icon class="size-4" />{table.title}
          {#if loading}<Spinner class="size-3.5" />{/if}
        </h3>
        <Segmented
          options={SORTS}
          bind:value={() => sortBy, (by) => (picked = { range, by })}
          label={`Sort ${table.title.toLowerCase()} by`}
          class="ml-auto @min-[19rem]:hidden" />
      </div>
      <table class="data-table dense w-full table-fixed text-sm tabular-nums">
        <!-- the widths are set here, where the font is the cells', so a width in ch is theirs and
             not the smaller header's; a phone leaves out the speed and the browse button, the
             name's menu having Browse shares, so the name keeps room -->
        <colgroup>
          <col class="w-8" />
          <col />
          {#each COLUMNS as c (c.by)}
            <col
              class={[c.phone === false && "max-md:hidden", "@max-[19rem]:hidden"]}
              style:width={columnWidth(c.widest)} />
          {/each}
          <col class="w-12 max-md:hidden" />
        </colgroup>
        <thead>
          <tr>
            <th class="text-right">#</th>
            <th class="truncate">Username</th>
            {#each COLUMNS as c (c.by)}
              <th
                class={[
                  "overflow-hidden text-right @max-[19rem]:hidden",
                  c.phone === false && "max-md:hidden"
                ]}
                aria-sort={sortBy === c.by ? "descending" : "none"}>
                <Tooltip text={c.title}>
                  {#snippet child({ props })}
                    <button
                      {...props}
                      type="button"
                      class="inline-flex max-w-full items-center gap-1"
                      onclick={() => (picked = { range, by: c.by })}>
                      <span class="truncate">{c.label}</span>
                      {#if sortBy === c.by}<ArrowDownIcon class="size-3 shrink-0" />{/if}
                    </button>
                  {/snippet}
                </Tooltip>
              </th>
            {/each}
            <th class="max-md:hidden"><span class="sr-only">{USER_ACTIONS.shares.label}</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows[table.direction] as entry, i (entry.username)}
            <tr>
              <td class="text-right text-fg-muted tabular-nums">{i + 1}</td>
              <td class="overflow-hidden">
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
                    "text-right tabular-nums @max-[19rem]:hidden",
                    c.phone === false && "max-md:hidden"
                  ]}>{c.value(entry)}</td>
              {/each}
              <td class="py-0.5 max-md:hidden">
                {#each reach(entry.username, { only: ["shares"] }) as action (action.id)}
                  <Tooltip text={action.of(entry.username)}>
                    {#snippet child({ props })}
                      <Button
                        {...props}
                        variant="ghost"
                        size="icon-sm"
                        href={action.href}
                        aria-label={action.of(entry.username)}><action.icon /></Button>
                    {/snippet}
                  </Tooltip>
                {/each}
              </td>
            </tr>
          {:else}
            <tr
              ><td colspan="6" class="py-6 text-center text-fg-subtle"
                >No {table.title.toLowerCase()} in this range</td
              ></tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/each}
</div>
