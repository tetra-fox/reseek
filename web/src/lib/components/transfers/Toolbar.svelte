<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import BrushCleaningIcon from "@lucide/svelte/icons/brush-cleaning";
  import Columns3Icon from "@lucide/svelte/icons/columns-3";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import XIcon from "@lucide/svelte/icons/x";

  import { MARK_FILTERS, type MarkFilter } from "#lib/analysis/marks.js";
  import { analyzerOffered } from "#lib/analysis/marks.svelte.js";
  import { app } from "#lib/app.svelte.js";
  import InspectorToggle from "#lib/components/inspector/InspectorToggle.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import Input from "#lib/components/ui/Input.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import SplitButton from "#lib/components/ui/SplitButton.svelte";
  import { formatBytes, formatSpeed, plural } from "#lib/format.js";
  import { type ActiveFilter, TEXT_FILTER } from "#lib/lists/filtered.js";
  import { session } from "#lib/session.svelte.js";
  import { type BulkAction, SCOPES } from "#lib/transfers/bulk.js";
  import { COLUMNS, columnsFor } from "#lib/transfers/grid/columns.js";
  import { defaults, type ViewSettings } from "#lib/transfers/grid/settings.svelte.js";
  import { type GroupBy, type SortKey, TRANSFER_PLACEHOLDER } from "#lib/transfers/grid/view.js";
  import { type ChipId, type Direction, isSucceeded, STATE_CHIPS } from "#lib/transfers/states.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  type Props = {
    direction: Direction;
    view: ViewSettings;
    /** how many transfers each state chip stands for, and all of them */
    counts: Record<ChipId | "all", number>;
    /** the filters on, the state and analysis chips' and the text's */
    filters: ActiveFilter[];
    /** how many downloads each analysis chip stands for; null on the uploads screen */
    markCounts: Record<MarkFilter, number> | null;
    text: string;
    /** a bulk action is running, or there is nothing to act on */
    disabled: boolean;
    onbulk: (action: BulkAction, test: (state: string) => boolean, scope: string) => void;
    onadd: () => void;
    /** opens the message to every user in the list, on the uploads screen */
    onmessage: () => void;
  };

  let {
    direction,
    view,
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
  const DENSITIES = [
    { id: "comfortable", label: "Comfortable" },
    { id: "compact", label: "Compact" }
  ] as const;
  const GROUPINGS: { id: GroupBy; label: string }[] = $derived([
    { id: "user", label: "User › folder" },
    { id: "folder", label: "Folder" },
    ...(direction === "download" ? [{ id: "batch" as const, label: "Batch" }] : []),
    { id: "flat", label: "Flat" }
  ]);

  // every order a column header can sort by, named after the first column that sorts by it; not
  // the user while each user is a group, inside which their order has no effect, as the table
  // hides that column then
  const SORTS = $derived(
    columnsFor(direction)
      .flatMap((c) => (c.sort ? [{ id: c.sort, label: c.label }] : []))
      .filter((s, i, all) => all.findIndex((o) => o.id === s.id) === i)
      .filter((s) => !(s.id === "user" && view.groupBy === "user"))
  );
  const sortLabel = $derived(SORTS.find((s) => s.id === view.sort.id)?.label ?? view.sort.id);

  /** the filters behind the phone's Filters button: the chips', not the text box's beside it */
  const chipFilters = $derived(filters.filter((f) => f.id !== TEXT_FILTER));

  const reset = () => Object.assign(view, defaults(direction));

  const toggleChip = (id: ChipId) => {
    const states = view.filters.states;
    view.filters.states = states.includes(id) ? states.filter((s) => s !== id) : [...states, id];
  };

  const scoped = (action: BulkAction, id: string) => {
    const scope = SCOPES[action].find((s) => s.id === id)!;
    onbulk(action, scope.test, scope.id);
  };
</script>

<div class="flex flex-col gap-2">
  <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
    <Filter.Panel filters={chipFilters}>
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
    <Segmented options={GROUPINGS} bind:value={view.groupBy} label="Group by" size="sm" />
    <Segmented options={DENSITIES} bind:value={view.density} label="Density" size="sm" />
    <Menu.Root>
      <Menu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="outline" size="sm">
            {#if view.sort.dir === "asc"}<ArrowUpIcon />{:else}<ArrowDownIcon />{/if}Sort: {sortLabel}
          </Button>
        {/snippet}
      </Menu.Trigger>
      <Menu.Content align="start" class="min-w-48">
        <Menu.RadioGroup
          aria-label="Sort by"
          bind:value={
            () => view.sort.id, (id) => (view.sort = { id: id as SortKey, dir: view.sort.dir })
          }>
          {#each SORTS as s (s.id)}
            <Menu.RadioItem value={s.id} closeOnSelect={false}>{s.label}</Menu.RadioItem>
          {/each}
        </Menu.RadioGroup>
        <Menu.Separator />
        <Menu.RadioGroup
          aria-label="Direction"
          bind:value={
            () => view.sort.dir,
            (dir) => (view.sort = { id: view.sort.id, dir: dir as "asc" | "desc" })
          }>
          <Menu.RadioItem value="asc" closeOnSelect={false}>Ascending</Menu.RadioItem>
          <Menu.RadioItem value="desc" closeOnSelect={false}>Descending</Menu.RadioItem>
        </Menu.RadioGroup>
        {#if viewport.phone}
          <!-- a phone has no Columns menu, which holds this on a wider screen -->
          <Menu.Separator />
          <Menu.Item onSelect={reset}>Reset view</Menu.Item>
        {/if}
      </Menu.Content>
    </Menu.Root>
    <!-- a phone shows a fixed few columns, so it has nothing to choose -->
    {#if !viewport.phone}
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="outline" size="sm"><Columns3Icon />Columns</Button>
          {/snippet}
        </Menu.Trigger>
        <Menu.Content align="start" class="min-w-48">
          {#each view.columns as column (column.id)}
            {@const known = COLUMNS.find((c) => c.id === column.id)}
            {#if known && !known.always}
              <Menu.Item
                closeOnSelect={false}
                onSelect={() => (column.visible = !column.visible)}
                aria-checked={column.visible}
                role="menuitemcheckbox">
                <span
                  class={["rounded-sm size-3 border", column.visible && "border-accent bg-accent"]}
                ></span>
                {known.label}
              </Menu.Item>
            {/if}
          {/each}
          <Menu.Separator />
          <Menu.Item onSelect={reset}>Reset view</Menu.Item>
        </Menu.Content>
      </Menu.Root>
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
          variant="danger"
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
          onclick={() => onbulk("remove", isSucceeded, "succeeded")}>
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
      {metrics.inProgress.files} active · {formatSpeed(metrics.inProgress.totalSpeed)} ·
      {metrics.queued.files} queued · {metrics.completed.failed} failed · {formatBytes(
        metrics.completed.bytes
      )} since reseek started
    </p>
  {/if}
</div>
