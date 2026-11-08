<script lang="ts">
  import FilterIcon from "@lucide/svelte/icons/filter";

  import ColumnsMenu from "#lib/components/files/ColumnsMenu.svelte";
  import ExpandAll from "#lib/components/files/ExpandAll.svelte";
  import LayoutToggle from "#lib/components/files/LayoutToggle.svelte";
  import SortMenu from "#lib/components/files/SortMenu.svelte";
  import InspectorToggle from "#lib/components/inspector/InspectorToggle.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import Input from "#lib/components/ui/Input.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { FILTER_PLACEHOLDER } from "#lib/files/filters.js";
  import { formatCount, plural } from "#lib/format.js";
  import type { ActiveFilter } from "#lib/lists/filtered.js";
  import {
    RESULT_COLUMNS,
    type ResultColumn,
    resultList,
    resultListDefaults
  } from "#lib/search/list.js";
  import { HIDING } from "#lib/search/results.js";
  import { type ResultSort, searchOptions } from "#lib/settings.svelte.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";

  import FilterHelp from "./FilterHelp.svelte";
  import FilterPresets from "./FilterPresets.svelte";

  type Props = {
    filter: string;
    /** what the filter and toggles leave, out of everything that came */
    counts: { files: number; users: number; allFiles: number; allUsers: number };
    /** the filters on, the Hide chips' and the text's */
    filters: ActiveFilter[];
    /** results are still coming */
    running: boolean;
    /** the results are all in, so sorting them moves nothing under the reader */
    sortable: boolean;
    /** how many users are blocked, whose results show only when asked */
    blockedCount: number;
    showBlocked: boolean;
    /** which users and folders of the tree layout are open */
    expansion: Expansion;
  };

  let {
    filter = $bindable(),
    counts,
    filters,
    running,
    sortable,
    blockedCount,
    showBlocked = $bindable(),
    expansion
  }: Props = $props();

  /** the tree's sorts, each of which names its own direction */
  const TREE_SORTS: { id: ResultSort; label: string }[] = [
    { id: "uploadSpeed", label: "Fastest upload speed" },
    { id: "queueLength", label: "Shortest queue" }
  ];
  /** the list's sorts, the columns whose headers sort */
  const LIST_SORTS = RESULT_COLUMNS.flatMap((c) =>
    c.value ? [{ id: c.id, label: c.label, descFirst: c.descFirst }] : []
  );
  const OPTIONAL = RESULT_COLUMNS.filter((c) => !c.always);
</script>

<div class="flex flex-col gap-2.5">
  <div class="flex flex-wrap items-center gap-x-inline gap-y-2">
    <LayoutToggle bind:value={searchOptions.layout} />
    {#if searchOptions.layout === "tree"}
      <SortMenu
        options={TREE_SORTS}
        sort={{ id: searchOptions.sort }}
        onsort={(sort) => (searchOptions.sort = sort.id as ResultSort)}
        directed={false}
        disabled={!sortable} />
      <ExpandAll {expansion} />
    {:else}
      <SortMenu
        options={LIST_SORTS}
        sort={resultList.sort}
        onsort={(sort) => (resultList.sort = { id: sort.id as ResultColumn, dir: sort.dir })} />
      <ColumnsMenu
        columns={OPTIONAL.map((c) => ({
          id: c.id,
          label: c.label,
          visible: !resultList.hidden.includes(c.id as ResultColumn)
        }))}
        ontoggle={(id, visible) => {
          const hidden = resultList.hidden.filter((h) => h !== id);
          resultList.hidden = visible ? hidden : [...hidden, id as ResultColumn];
        }}
        onreset={() => Object.assign(resultList, resultListDefaults())} />
    {/if}
    <InspectorToggle />
    <p class="ml-auto flex items-center gap-1.5 text-sm text-fg-muted tabular-nums" role="status">
      {#if running}<Spinner class="size-3.5" />{/if}
      <!-- with a filter on, the line of filters under the toolbar carries the count -->
      {#if !filters.length}
        {plural(counts.files, "file", "files")} from {plural(counts.users, "user", "users")}
        {#if counts.users !== counts.allUsers}
          <span class="text-fg-subtle">
            (of {formatCount(counts.allFiles)} from {formatCount(counts.allUsers)})
          </span>
        {/if}
      {/if}
    </p>
  </div>
  <div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
    <Filter.Panel {filters}>
      <Filter.Group label="Hide">
        {#each HIDING as h (h.key)}
          <Tooltip text={h.hint}>
            {#snippet child({ props })}
              <Filter.Chip
                {...props}
                pressed={searchOptions[h.key]}
                onclick={() => (searchOptions[h.key] = !searchOptions[h.key])}
                >{h.chip}</Filter.Chip>
            {/snippet}
          </Tooltip>
        {/each}
      </Filter.Group>
    </Filter.Panel>
    {#if blockedCount}
      <label class="flex items-center gap-2">
        <Switch bind:checked={showBlocked} />Show blocked users
      </label>
    {/if}
  </div>
  <div class="flex items-center gap-1">
    <Input
      bind:value={filter}
      type="search"
      icon={FilterIcon}
      aria-label="Filter"
      placeholder={FILTER_PLACEHOLDER}
      class={["font-mono text-xs md:text-xs", filter.trim() && Filter.FILTERING]} />
    <FilterHelp />
  </div>
  <FilterPresets bind:filter />
</div>
