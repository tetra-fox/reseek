<script lang="ts">
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import EraserIcon from "@lucide/svelte/icons/eraser";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import PlayIcon from "@lucide/svelte/icons/play";

  import { api, type Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import Input from "#lib/components/ui/Input.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { type ActiveFilter, typedFilter } from "#lib/lists/filtered.js";
  import { freezesWhileUsed, interaction } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { call } from "#lib/request.js";
  import { LEVELS, type Line, logLine, logTime, numbered } from "#lib/system/logs.js";

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
  const asText = (picked: Line[]) =>
    picked
      .map(
        (l) => `${l.record.timestamp} ${l.record.level} ${l.record.context}: ${l.record.message}`
      )
      .join("\n");
  const copyChosen = () => void copyText(asText(chosen));
  /** Copies the newest `n` lines the filters show, oldest first, as a log file reads */
  const copyLast = (n: number) => void copyText(asText(shown.slice(0, n).toReversed()));
  let grid = $state<HTMLElement | null>(null);
  /** the levels hidden, then the text */
  const filters = $derived<ActiveFilter[]>([
    ...LEVELS.filter((l) => !levels.includes(l.level)).map((l) => ({
      id: l.level,
      label: `${l.level} hidden`,
      clear: () => (levels = [...levels, l.level])
    })),
    ...typedFilter(text, () => (text = ""))
  ]);
  /** how many lines of each level the list holds, for the level chips */
  const counts = $derived(
    lines.reduce<Record<string, number>>((all, l) => {
      all[l.record.level] = (all[l.record.level] ?? 0) + 1;
      return all;
    }, {})
  );
</script>

<div class="flex flex-wrap items-center gap-inline">
  {#if paused}
    <Button size="sm" onclick={resume}
      ><PlayIcon />Resume{waiting.length ? ` (${waiting.length} new)` : ""}</Button>
  {:else}
    <Button size="sm" variant="outline" onclick={() => (paused = true)}><PauseIcon />Pause</Button>
  {/if}
  <Filter.Group label="Levels">
    {#each LEVELS as l (l.level)}
      {@const on = levels.includes(l.level)}
      <Filter.Chip
        pressed={on}
        hides
        count={counts[l.level] ?? 0}
        class="font-mono text-xs"
        title={on
          ? `Hide ${l.level} lines. Alt-click to show only these.`
          : `Show ${l.level} lines`}
        onclick={(e) =>
          (levels = e.altKey
            ? [l.level]
            : on
              ? levels.filter((x) => x !== l.level)
              : [...levels, l.level])}>{l.short}</Filter.Chip>
    {/each}
  </Filter.Group>
  <Input
    type="search"
    wrapperClass="w-56"
    class={["h-control-sm", text.trim() && Filter.FILTERING]}
    placeholder="Filter"
    aria-label="Filter the log"
    bind:value={text} />
  <div class="ml-auto flex gap-inline">
    {#if chosen.length}
      <Button size="sm" variant="outline" onclick={copyChosen}
        ><CopyIcon />Copy {chosen.length}</Button>
    {/if}
    <Menu.Root>
      <Menu.Trigger disabled={!shown.length}>
        {#snippet child({ props })}
          <Button {...props} size="sm" variant="outline"
            ><CopyIcon />Copy last<ChevronDownIcon /></Button>
        {/snippet}
      </Menu.Trigger>
      <Menu.Content align="end">
        {#each [50, 100, 500] as n (n)}
          <Menu.Item onSelect={() => copyLast(n)}>{n} lines</Menu.Item>
        {/each}
      </Menu.Content>
    </Menu.Root>
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

<Filter.Summary
  {filters}
  shown={shown.length}
  total={lines.length}
  noun={["line", "lines"]}
  refocus={() => grid?.focus()} />

<div
  bind:this={grid}
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
      class={["grid grid-cols-[4.5rem_2.5rem_1fr] gap-2 list-row px-2 py-0.5", level?.row]}>
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
    {#if lines.length}
      <Filter.Empty
        {filters}
        total={lines.length}
        noun={["line", "lines"]}
        refocus={() => grid?.focus()} />
    {:else}
      <EmptyRow text="No log lines" />
    {/if}
  {/each}
</div>
