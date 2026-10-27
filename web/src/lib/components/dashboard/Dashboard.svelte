<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import SearchBar from "#lib/components/search/SearchBar.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { dashboardPrefs } from "#lib/dashboard/prefs.svelte.js";
  import { chartData, RANGES, reports, windowOf } from "#lib/dashboard/reports.js";
  import { SERIES } from "#lib/dashboard/series.js";
  import { formatBytes, formatSpeed } from "#lib/format.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";

  import AreaChart from "./AreaChart.svelte";
  import Errors from "./Errors.svelte";
  import Leaderboard from "./Leaderboard.svelte";
  import StatTiles from "./StatTiles.svelte";
  import TopDirectories from "./TopDirectories.svelte";

  // the home page: a search, what moves now, and the history of the chosen range
  type Tab = "users" | "content" | "errors";

  /** the window ends when it was chosen or last reloaded, not at every render */
  let now = $state(Date.now());
  const span = $derived(windowOf(dashboardPrefs.range, now));
  keepCurrent(() => (now = Date.now()));

  let summary = $state.raw<Schemas["SummaryReport"] | null>(null);
  let summaryError = $state("");
  let histogram = $state.raw<Record<string, Schemas["DirectionSummaries"]> | null>(null);
  let histogramError = $state("");
  let directories = $state.raw<Schemas["DirectoryCount"][]>([]);
  let directoriesError = $state("");
  let tab = $state<Tab>("users");
  const visible = new SvelteSet(["uploadBytes", "downloadBytes"]);

  // each report loads on its own, so one failing never blanks the others; what loaded before
  // stays until the new answer is here
  let asked = 0;
  $effect(() => {
    const w = span;
    const at = ++asked;
    untrack(() => {
      void reports.summary(w).then((o) => {
        if (at !== asked) return;
        if (o.ok) [summary, summaryError] = [o.data, ""];
        else summaryError = o.error;
      });
      void reports.histogram(w, w.buckets).then((o) => {
        if (at !== asked) return;
        if (o.ok) [histogram, histogramError] = [o.data, ""];
        else histogramError = o.error;
      });
      void reports.directories(w).then((o) => {
        if (at !== asked) return;
        if (o.ok) [directories, directoriesError] = [o.data, ""];
        else directoriesError = o.error;
      });
    });
  });

  const data = $derived(histogram ? chartData(histogram) : []);
  const metrics = $derived(app.metrics);
</script>

<SearchBar announce />

{#if metrics}
  <div class="grid gap-section sm:grid-cols-2">
    {#each [{ label: "Downloading", m: metrics.downloads, Icon: ArrowDownIcon, tone: "text-info" }, { label: "Uploading", m: metrics.uploads, Icon: ArrowUpIcon, tone: "text-success" }] as row (row.label)}
      <div class="flex items-center gap-4 rounded-card border bg-surface p-4 shadow-card">
        <row.Icon class={["size-5", row.tone]} />
        <div class="flex flex-col">
          <span class="text-lg font-semibold tabular-nums"
            >{formatSpeed(row.m.inProgress.totalSpeed)}</span>
          <span class="text-xs text-fg-muted">
            {row.label}: {row.m.inProgress.files} active, {row.m.queued.files} queued ({formatBytes(
              row.m.queued.bytes
            )}), {row.m.completed.succeeded} done and {row.m.completed.failed} failed since start
          </span>
        </div>
      </div>
    {/each}
  </div>
{/if}

<div class="flex flex-wrap items-center gap-1" role="group" aria-label="Range">
  {#each RANGES as r (r.label)}
    <button
      type="button"
      aria-pressed={dashboardPrefs.range === r.label}
      class="h-control-sm rounded-control px-2.5 text-sm text-fg-muted hover:bg-hover aria-pressed:bg-surface-muted aria-pressed:font-medium aria-pressed:text-fg"
      onclick={() => {
        dashboardPrefs.range = r.label;
        now = Date.now();
      }}>{r.label}</button>
  {/each}
</div>

{#if summaryError}<p class="text-sm text-danger">Couldn't load the totals: {summaryError}</p>{/if}
{#if summary}
  <StatTiles {summary} />
{:else if !summaryError}
  <Skeleton class="h-24" />
{/if}

<Card title="History">
  {#if histogramError}<p class="text-sm text-danger">
      Couldn't load the history: {histogramError}
    </p>{/if}
  {#if histogram}
    <AreaChart {data} series={SERIES} {visible} label="Transfers over time" />
  {:else if !histogramError}
    <Skeleton class="h-52" />
  {/if}
</Card>

<nav aria-label="Statistics" class="flex gap-1">
  {#each [{ id: "users", title: "Users" }, { id: "content", title: "Content" }, { id: "errors", title: "Errors" }] as t (t.id)}
    <button
      type="button"
      aria-current={tab === t.id ? "page" : undefined}
      class="flex h-control items-center rounded-control px-3 text-sm text-fg-muted hover:bg-hover hover:text-fg aria-[current]:nav-current"
      onclick={() => (tab = t.id as Tab)}>{t.title}</button>
  {/each}
</nav>

<Card>
  {#if tab === "users"}
    <Leaderboard {span} range={dashboardPrefs.range} />
  {:else if tab === "content"}
    {#if directoriesError}<p class="text-sm text-danger">Couldn't load: {directoriesError}</p>{/if}
    <TopDirectories rows={directories} />
  {:else}
    <Errors {span} {data} />
  {/if}
</Card>
