<script lang="ts">
  import BrushCleaningIcon from "@lucide/svelte/icons/brush-cleaning";
  import GroupIcon from "@lucide/svelte/icons/group";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import XIcon from "@lucide/svelte/icons/x";

  import { MARK_FILTERS, type MarkFilter } from "#lib/analysis/marks.js";
  import { analyzerOffered } from "#lib/analysis/marks.svelte.js";
  import { app } from "#lib/app.svelte.js";
  import ColumnsMenu from "#lib/components/files/ColumnsMenu.svelte";
  import ExpandAll from "#lib/components/files/ExpandAll.svelte";
  import LayoutToggle from "#lib/components/files/LayoutToggle.svelte";
  import SortMenu from "#lib/components/files/SortMenu.svelte";
  import InspectorToggle from "#lib/components/inspector/InspectorToggle.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import Input from "#lib/components/ui/Input.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import SplitButton from "#lib/components/ui/SplitButton.svelte";
  import { formatBytes, formatCount, formatSpeed, plural } from "#lib/format.js";
  import type { ActiveFilter } from "#lib/lists/filtered.js";
  import { session } from "#lib/session.svelte.js";
  import { type BulkAction, SCOPES } from "#lib/transfers/bulk.js";
  import { COLUMNS, columnsFor } from "#lib/transfers/grid/columns.js";
  import { defaults, type ViewSettings } from "#lib/transfers/grid/settings.svelte.js";
  import {
    type GroupBy,
    groupingOf,
    type SortKey,
    TRANSFER_PLACEHOLDER
  } from "#lib/transfers/grid/view.js";
  import {
    type ChipId,
    type Direction,
    isSucceeded,
    STATE_CHIPS,
    type TransferState
  } from "#lib/transfers/states.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  type Props = {
    direction: Direction;
    view: ViewSettings;
    /** which groups of the tree layout are open */
    expansion: Expansion;
    /** how many transfers each state chip stands for, and all of them */
    counts: Record<ChipId | "all", number>;
    /** the filters on, the state and analysis chips' and the text's */
    filters: ActiveFilter[];
    /** how many downloads each analysis chip stands for; null on the uploads screen */
    markCounts: Record<MarkFilter, number> | null;
    text: string;
    /** a bulk action is running, or there is nothing to act on */
    disabled: boolean;
    onbulk: (action: BulkAction, test: (state: TransferState) => boolean) => void;
    onadd: () => void;
    /** opens the message to every user in the list, on the uploads screen */
    onmessage: () => void;
  };

  let {
    direction,
    view,
    expansion,
    counts,
    filters,
    markCounts,
    text = $bindable(),
    disabled,
    onbulk,
    onadd,
    onmessage
  }: Props = $props();

  let retry = $state<string>(SCOPES.retry[0].id);
  let cancel = $state<string>(SCOPES.cancel[0].id);
  let remove = $state<string>(SCOPES.remove[0].id);

  const write = $derived(session.can("ReadWrite"));
  const metrics = $derived(app.metrics?.[direction === "download" ? "downloads" : "uploads"]);
  const GROUPINGS: { id: GroupBy; label: string }[] = $derived([
    { id: "user", label: "User › folder" },
    { id: "folder", label: "Folder" },
    ...(direction === "download" ? [{ id: "job" as const, label: "Queued together" }] : [])
  ]);

  // every order a column header can sort by, named after the first column that sorts by it; not
  // the user while each user is a group, inside which their order has no effect, as the table
  // hides that column then
  const SORTS = $derived(
    columnsFor(direction)
      .flatMap((c) => (c.sort ? [{ id: c.sort, label: c.label }] : []))
      .filter((s, i, all) => all.findIndex((o) => o.id === s.id) === i)
      .filter((s) => !(s.id === "user" && groupingOf(view) === "user"))
  );

  const reset = () => Object.assign(view, defaults(direction));

  const toggleChip = (id: ChipId) => {
    const states = view.filters.states;
    view.filters.states = states.includes(id) ? states.filter((s) => s !== id) : [...states, id];
  };

  const scoped = (action: keyof typeof SCOPES, id: string) => {
    const scope = SCOPES[action].find((s) => s.id === id)!;
    onbulk(action, scope.test);
  };
</script>

<div class="flex flex-col gap-2">
  <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
    <Filter.Panel {filters}>
      <Filter.Group label="State">
        {#each STATE_CHIPS as chip (chip.id)}
          <Filter.Chip
            pressed={view.filters.states.includes(chip.id)}
            count={counts[chip.id]}
            onclick={() => toggleChip(chip.id)}>{chip.label}</Filter.Chip>
        {/each}
      </Filter.Group>
      {#if markCounts && analyzerOffered()}
        <Filter.Group label="Analysis">
          {#each MARK_FILTERS as f (f.id)}
            {@const on = view.filters.analysis === f.id}
            <Filter.Chip
              pressed={on}
              count={markCounts[f.id]}
              onclick={() => (view.filters.analysis = on ? null : f.id)}>{f.label}</Filter.Chip>
          {/each}
        </Filter.Group>
      {/if}
    </Filter.Panel>
    <div class="relative min-w-48 flex-1">
      <Input
        bind:value={text}
        type="search"
        aria-label="Filter"
        placeholder={TRANSFER_PLACEHOLDER}
        class={["h-control-sm", text.trim() && Filter.FILTERING]} />
    </div>
    <!-- with a filter on, the line of filters under the toolbar carries the count -->
    {#if !filters.length}
      <span class="text-sm text-fg-muted tabular-nums">
        {direction === "download"
          ? plural(counts.all, "download", "downloads")
          : plural(counts.all, "upload", "uploads")}
      </span>
    {/if}
  </div>
  <div class="flex flex-wrap items-center gap-inline">
    <LayoutToggle bind:value={view.layout} />
    {#if view.layout === "tree" && viewport.phone}
      <!-- a phone has no room for the groupings side by side; the values bits-ui gives back are
           only their ids -->
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="outline" size="sm">
              <GroupIcon />Group: {GROUPINGS.find((g) => g.id === view.groupBy)?.label}
            </Button>
          {/snippet}
        </Menu.Trigger>
        <Menu.Content align="start" class="min-w-48">
          <Menu.RadioGroup
            aria-label="Group by"
            bind:value={() => view.groupBy, (id) => (view.groupBy = id as GroupBy)}>
            {#each GROUPINGS as g (g.id)}
              <Menu.RadioItem value={g.id}>{g.label}</Menu.RadioItem>
            {/each}
          </Menu.RadioGroup>
        </Menu.Content>
      </Menu.Root>
    {:else if view.layout === "tree"}
      <Segmented options={GROUPINGS} bind:value={view.groupBy} label="Group by" size="sm" />
    {/if}
    <SortMenu
      options={SORTS}
      sort={view.sort}
      onsort={(sort) => (view.sort = { id: sort.id as SortKey, dir: sort.dir })}>
      {#if viewport.phone}
        <!-- a phone has no Columns menu, which holds this on a wider screen -->
        <Menu.Item onSelect={reset}>Reset view</Menu.Item>
      {/if}
    </SortMenu>
    <!-- a phone shows a fixed few columns, so it has nothing to choose -->
    {#if !viewport.phone}
      <ColumnsMenu
        columns={view.columns.flatMap((column) => {
          const known = COLUMNS.find((c) => c.id === column.id);
          return known && !known.always
            ? [{ id: column.id, label: known.label, visible: column.visible }]
            : [];
        })}
        ontoggle={(id, visible) => (view.columns.find((c) => c.id === id)!.visible = visible)}
        onreset={reset} />
    {/if}
    {#if view.layout === "tree"}
      <ExpandAll {expansion} />
    {/if}
    <InspectorToggle />
    {#if write}
      <div class="ml-auto flex flex-wrap items-center gap-inline">
        {#if direction === "download"}
          <SplitButton
            options={SCOPES.retry}
            bind:chosen={retry}
            icon={RotateCwIcon}
            disabled={disabled || !app.isServerConnected}
            onclick={(id) => scoped("retry", id)} />
        {/if}
        <SplitButton
          options={SCOPES.cancel}
          bind:chosen={cancel}
          icon={XIcon}
          {disabled}
          onclick={(id) => scoped("cancel", id)} />
        <SplitButton
          options={SCOPES.remove}
          bind:chosen={remove}
          icon={Trash2Icon}
          {disabled}
          onclick={(id) => scoped("remove", id)} />
        <Button
          variant="outline"
          size="sm"
          disabled={disabled || !counts.completed}
          onclick={() => onbulk("remove", isSucceeded)}>
          <BrushCleaningIcon />Clear succeeded
        </Button>
        {#if direction === "upload"}
          <Button variant="outline" size="sm" {disabled} onclick={onmessage}>
            <MessageSquareIcon />Message all
          </Button>
        {/if}
        {#if direction === "download"}
          <Button size="sm" onclick={onadd}><PlusIcon />Add</Button>
        {/if}
      </div>
    {/if}
  </div>
  {#if metrics}
    <p class="text-xs text-fg-muted tabular-nums">
      {formatCount(metrics.inProgress.files)} active · {formatSpeed(metrics.inProgress.totalSpeed)} ·
      {formatCount(metrics.queued.files)} queued · {formatBytes(metrics.completed.bytes)}
      {direction === "download" ? "downloaded" : "uploaded"}, {formatCount(
        metrics.completed.failed
      )}
      failed since reseek started
    </p>
  {/if}
</div>
