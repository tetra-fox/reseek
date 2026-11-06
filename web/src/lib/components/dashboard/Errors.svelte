<script lang="ts">
  import { untrack } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import { api } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import {
    type Direction,
    merged,
    type Point,
    reports,
    type Window
  } from "#lib/dashboard/reports.js";
  import { ERRORS } from "#lib/dashboard/series.js";
  import { formatBytes, formatDate, formatSpeed, getFileName, truncate } from "#lib/format.js";
  import { call } from "#lib/request.js";
  import { endingText } from "#lib/transfers/states.js";

  import AreaChart from "./AreaChart.svelte";

  // failed transfers: over time, by error, and the latest ones
  let { span, data }: { span: Window; data: Point[] } = $props();

  type Which = Direction | "All";
  type Pareto = Schemas["ExceptionCount"] & { direction: Direction };
  type Recent = Schemas["TransferExceptionDetail"];

  let hidden = $state<Direction[]>([]);
  let paretoWhich = $state<Which>("All");
  let recentWhich = $state<Which>("All");
  let exception = $state<string | null>(null);
  let pareto = $state.raw<Pareto[]>([]);
  let recent = $state.raw<Recent[]>([]);
  let paretoError = $state("");
  let recentError = $state("");
  let detail = $state<Recent | null>(null);

  const directions = (which: Which): Direction[] =>
    which === "All" ? ["Upload", "Download"] : [which];

  let paretoAsk = 0;
  $effect(() => {
    const w = span;
    const which = paretoWhich;
    const at = ++paretoAsk;
    untrack(async () => {
      const answers = await Promise.all(directions(which).map((d) => reports.pareto(w, d)));
      if (at !== paretoAsk) return;
      const failed = answers.find((o) => !o.ok);
      if (failed && !failed.ok) return void (paretoError = failed.error);
      paretoError = "";
      const [first, second] = answers.map((o) => (o.ok ? o.data : []));
      pareto =
        which === "All"
          ? merged(first, second, (a, b) => b.count - a.count)
          : first.map((r) => ({ ...r, direction: which }));
    });
  });

  let recentAsk = 0;
  $effect(() => {
    const w = span;
    const which = recentWhich;
    const only = exception;
    const at = ++recentAsk;
    untrack(async () => {
      const answers = await Promise.all(
        directions(which).map((direction) => reports.exceptions(w, direction, only ?? undefined))
      );
      if (at !== recentAsk) return;
      const failed = answers.find((o) => !o.ok);
      if (failed && !failed.ok) return void (recentError = failed.error);
      recentError = "";
      recent = answers
        .flatMap((o) => (o.ok ? o.data : []))
        .toSorted((a, b) => Date.parse(b.endedAt) - Date.parse(a.endedAt))
        .slice(0, 10);
    });
  });

  const top = $derived(pareto[0]?.count || 1);

  /** the direction toggles' words, the values being the API's directions */
  const WHICH: Record<Which, string> = { Upload: "Uploads", Download: "Downloads", All: "All" };
  /** how a failed transfer ended, as the start of its line: "Failed", "Timed out" */
  const endingWord = (state: string) => {
    const word = endingText(state);
    return word.charAt(0).toUpperCase() + word.slice(1);
  };
</script>

{#snippet which(value: Which, set: (w: Which) => void, label: string)}
  <Segmented
    options={(["Upload", "Download", "All"] as const).map((w) => ({ id: w, label: WHICH[w] }))}
    bind:value={() => value, set}
    {label}
    size="sm" />
{/snippet}

<AreaChart {data} view={ERRORS} bind:hidden label="Errors over time" />

<section class="flex flex-col gap-2">
  <div class="flex flex-wrap items-center gap-inline">
    <h3 class="text-sm font-medium">Errors by type</h3>
    {@render which(paretoWhich, (w) => (paretoWhich = w), "Direction of the error counts")}
  </div>
  {#if paretoError}<p class="text-sm text-danger">
      Could not load errors by type: {paretoError}
    </p>{/if}
  <!-- a phone scrolls the table sideways rather than squeeze the messages into slivers -->
  <div class="overflow-x-auto">
    <table class="w-full min-w-[36rem] text-sm">
      <thead class="text-xs text-fg-muted">
        <tr class="border-b">
          <th class="px-2 py-1 text-left font-medium">Direction</th>
          <th class="px-2 py-1 text-left font-medium">Error</th>
          <th class="w-24"></th>
          <th class="px-2 py-1 text-right font-medium">Count</th>
          <th class="px-2 py-1 text-right font-medium">Users</th>
        </tr>
      </thead>
      <tbody>
        {#each pareto as row (`${row.direction}-${row.exception}`)}
          <tr
            class={[
              "border-b last:border-b-0",
              exception === row.exception ? "bg-accent-soft/60" : "hover:bg-hover"
            ]}>
            <td class="px-2 py-1">{row.direction}</td>
            <td class="px-2 py-1" title={row.exception ?? ""}>
              <button
                type="button"
                class="text-left hover:underline"
                title="Show only this error in recent errors"
                onclick={() =>
                  (exception = exception === row.exception ? null : (row.exception ?? null))}
                >{truncate(row.exception ?? "(no message)", 80)}</button>
            </td>
            <td class="px-2">
              <div class="h-1.5 rounded-full bg-surface-muted">
                <div
                  class="h-full rounded-full bg-danger/60"
                  style:width={`${Math.round((row.count / top) * 100)}%`}>
                </div>
              </div>
            </td>
            <td class="px-2 py-1 text-right font-medium tabular-nums"
              >{row.count.toLocaleString()}</td>
            <td class="px-2 py-1 text-right tabular-nums">{row.distinctUsers.toLocaleString()}</td>
          </tr>
        {:else}
          <EmptyRow colspan={5} text="No errors in this range" />
        {/each}
      </tbody>
    </table>
  </div>
</section>

<section class="flex flex-col gap-2">
  <div class="flex flex-wrap items-center gap-inline">
    <h3 class="text-sm font-medium">Recent errors</h3>
    {@render which(recentWhich, (w) => (recentWhich = w), "Direction of the recent errors")}
    {#if exception}
      <Button size="sm" variant="ghost" onclick={() => (exception = null)}>Show all errors</Button>
    {/if}
  </div>
  {#if recentError}<p class="text-sm text-danger">
      Could not load recent errors: {recentError}
    </p>{/if}
  <div class="overflow-x-auto">
    <table class="w-full min-w-[36rem] text-sm">
      <thead class="text-xs text-fg-muted">
        <tr class="border-b">
          <th class="px-2 py-1 text-left font-medium">Time</th>
          <th class="px-2 py-1 text-left font-medium">Direction</th>
          <th class="px-2 py-1 text-left font-medium">Username</th>
          <th class="px-2 py-1 text-left font-medium">Filename</th>
          <th class="px-2 py-1 text-left font-medium">Error</th>
        </tr>
      </thead>
      <tbody>
        {#each recent as row (row.id)}
          <tr class="border-b last:border-b-0 hover:bg-hover">
            <td class="px-2 py-1 whitespace-nowrap">{formatDate(row.endedAt)}</td>
            <td class="px-2 py-1">{row.direction}</td>
            <td class="px-2 py-1"><Username name={row.username} /></td>
            <td class="px-2 py-1" title={row.filename}>
              <button type="button" class="text-left hover:underline" onclick={() => (detail = row)}
                >{getFileName(row.filename)}</button>
            </td>
            <td class="px-2 py-1" title={row.exception ?? ""}>
              <span class="font-medium">{endingWord(row.state)}</span>
              {#if row.exception}<span class="text-fg-muted">{truncate(row.exception, 80)}</span
                >{/if}
            </td>
          </tr>
        {:else}
          <EmptyRow colspan={5} text="No errors in this range" />
        {/each}
      </tbody>
    </table>
  </div>
</section>

<Dialog
  bind:open={() => detail !== null, (o) => !o && (detail = null)}
  title={detail ? getFileName(detail.filename) : ""}
  class="w-[min(40rem,calc(100vw-2rem))]">
  {#if detail}
    {@const d = detail}
    <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
      <dt class="text-fg-muted">User</dt>
      <dd><Username name={d.username} /></dd>
      <dt class="text-fg-muted">Direction</dt>
      <dd>{d.direction}</dd>
      <dt class="text-fg-muted">File</dt>
      <dd class="cursor-text font-mono text-xs wrap-anywhere">{d.filename}</dd>
      <dt class="text-fg-muted">Size</dt>
      <dd>{formatBytes(d.size)}, {formatBytes(d.bytesTransferred)} transferred</dd>
      <dt class="text-fg-muted">State</dt>
      <dd>{d.state}</dd>
      <dt class="text-fg-muted">Requested</dt>
      <dd>{formatDate(d.requestedAt)}</dd>
      {#if d.enqueuedAt}<dt class="text-fg-muted">Queued</dt>
        <dd>{formatDate(d.enqueuedAt)}</dd>{/if}
      {#if d.startedAt}<dt class="text-fg-muted">Started</dt>
        <dd>{formatDate(d.startedAt)}</dd>{/if}
      <dt class="text-fg-muted">Ended</dt>
      <dd>{formatDate(d.endedAt)}</dd>
      <dt class="text-fg-muted">Speed</dt>
      <dd>{formatSpeed(d.averageSpeed)}</dd>
      <dt class="text-fg-muted">Error</dt>
      <dd class="cursor-text font-mono text-xs wrap-anywhere">{d.exception ?? ""}</dd>
    </dl>
  {/if}
</Dialog>
