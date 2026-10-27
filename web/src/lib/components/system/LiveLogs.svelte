<script lang="ts">
  import CopyIcon from "@lucide/svelte/icons/copy";
  import EraserIcon from "@lucide/svelte/icons/eraser";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import PlayIcon from "@lucide/svelte/icons/play";

  import { api, type Schemas } from "#lib/api/client.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import { freezesWhileUsed, interaction } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { call } from "#lib/request.js";

  import { LEVELS, type Line, logLine, logTime, numbered } from "./logs";

  // the daemon's newest log records as they are written, newest first
  const MAX = 500;

  let lines = $state.raw<Line[]>([]);
  let paused = $state(false);
  let waiting = $state.raw<Line[]>([]);
  let levels = $state<Schemas["LogLevel"][]>(LEVELS.map((l) => l.level));
  let text = $state("");
  let error = $state("");

  /** lines Clear took away, still matched so a reload does not bring them back */
  let cleared: Line[] = [];
  /** how many lines the list keeps: all of them while the user holds it */
  const cap = () => (interaction.frozen ? undefined : MAX);

  // the snapshot a reopened stream sends replaces the list, keeping the rows of the records
  // already shown
  const take = (records: Schemas["LogRecord"][]) => {
    lines = numbered(records, [...waiting, ...lines])
      .toReversed()
      .slice(0, cap());
    waiting = [];
    cleared = [];
  };
  // a background read adds the records not shown yet, in time order, and removes nothing: the
  // daemon keeps fewer records than the list does
  const merge = (records: Schemas["LogRecord"][]) => {
    const known = [...waiting, ...lines, ...cleared];
    const seqs = new Set(known.map((l) => l.seq));
    const fresh = numbered(records, known).filter((l) => !seqs.has(l.seq));
    if (!fresh.length) return;
    lines = [...fresh.toReversed(), ...lines]
      .toSorted((a, b) => Date.parse(b.record.timestamp) - Date.parse(a.record.timestamp))
      .slice(0, cap());
  };
  const reload = async () => {
    const outcome = await call(() => api.GET("/api/v0/logs/live"));
    if (outcome.ok) {
      error = "";
      merge(outcome.data);
    } else error = outcome.error;
  };
  keepCurrent(
    () => {
      if (!paused) void reload();
    },
    () => "logs"
  );
  void reload();
  $effect(() => live.on("logs.buffer", take));
  $effect(() =>
    live.on("logs.record", (record) => {
      const line = logLine(record);
      if (paused) waiting = [line, ...waiting].slice(0, MAX);
      // the oldest rows go only once nobody is selecting in or pressing on the list
      else lines = [line, ...lines].slice(0, cap());
    })
  );

  const resume = () => {
    lines = [...waiting, ...lines].slice(0, MAX);
    waiting = [];
    paused = false;
  };

  const shown = $derived.by(() => {
    const t = text.trim().toLowerCase();
    return lines.filter(
      (l) =>
        levels.includes(l.record.level) &&
        (!t ||
          l.record.message.toLowerCase().includes(t) ||
          l.record.context.toLowerCase().includes(t))
    );
  });

  const list = new ListControl<Line>({
    lines: () => shown,
    key: (l) => String(l.seq),
    id: (l) => String(l.seq)
  });
  // the selection counts the rows the filters hide too, as the transfers grid does
  const chosen = $derived(lines.filter((l) => list.selected.has(String(l.seq))));
  const copyChosen = () =>
    void copyText(
      chosen
        .map(
          (l) => `${l.record.timestamp} ${l.record.level} ${l.record.context}: ${l.record.message}`
        )
        .join("\n")
    );
</script>

<div class="flex flex-wrap items-center gap-inline">
  {#if paused}
    <Button size="sm" onclick={resume}
      ><PlayIcon />Resume{waiting.length ? ` (${waiting.length} new)` : ""}</Button>
  {:else}
    <Button size="sm" variant="outline" onclick={() => (paused = true)}><PauseIcon />Pause</Button>
  {/if}
  <div class="flex flex-wrap gap-1" role="group" aria-label="Levels">
    {#each LEVELS as l (l.level)}
      <Button
        size="sm"
        variant={levels.includes(l.level) ? "secondary" : "outline"}
        aria-pressed={levels.includes(l.level)}
        onclick={() =>
          (levels = levels.includes(l.level)
            ? levels.filter((x) => x !== l.level)
            : [...levels, l.level])}>{l.short}</Button>
    {/each}
  </div>
  <Input
    class="h-control-sm w-56"
    placeholder="Filter"
    aria-label="Filter the log"
    bind:value={text} />
  <div class="ml-auto flex gap-inline">
    {#if chosen.length}
      <Button size="sm" variant="outline" onclick={copyChosen}
        ><CopyIcon />Copy {chosen.length}</Button>
    {/if}
    <Button
      size="sm"
      variant="ghost"
      onclick={() => {
        cleared = [...waiting, ...lines, ...cleared].slice(0, MAX);
        lines = [];
        waiting = [];
        list.clear();
      }}><EraserIcon />Clear</Button>
  </div>
</div>

{#if error}<Notice kind="error">{error}</Notice>{/if}

<div
  class="max-h-[65svh] overflow-auto rounded-card border bg-surface font-mono text-xs shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent"
  role="grid"
  aria-label="Live log"
  aria-multiselectable="true"
  aria-activedescendant={list.activeId}
  tabindex="0"
  {@attach list.attach}
  {@attach freezesWhileUsed}>
  {#each shown as l, i (l.seq)}
    {@const level = LEVELS.find((x) => x.level === l.record.level)}
    <div
      role="row"
      {...list.line(l, i)}
      class={[
        "grid grid-cols-[4.5rem_2.5rem_1fr] gap-2 border-b px-2 py-0.5 last:border-b-0",
        level?.row,
        list.selected.has(String(l.seq)) && "bg-accent-soft/60",
        list.isFocused(l) && "ring-1 ring-accent ring-inset"
      ]}>
      <span role="gridcell" class="text-fg-muted">{logTime(l.record.timestamp)}</span>
      <span role="gridcell">{level?.short ?? l.record.level}</span>
      <span role="gridcell" class="break-all whitespace-pre-wrap">
        <span class="text-fg-subtle" title={l.record.context}
          >{l.record.context.split("::").pop()}{l.record.subContext
            ? `[${l.record.subContext}]`
            : ""}</span>
        {l.record.message}
      </span>
    </div>
  {:else}
    <p class="px-4 py-6 text-center text-fg-subtle">No logs</p>
  {/each}
</div>
