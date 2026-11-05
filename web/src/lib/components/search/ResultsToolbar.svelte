<script lang="ts">
  import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
  import FilterIcon from "@lucide/svelte/icons/filter";
  import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
  import Rows3Icon from "@lucide/svelte/icons/rows-3";

  import InspectorToggle from "#lib/components/inspector/InspectorToggle.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import Input from "#lib/components/ui/Input.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import { FILTER_PLACEHOLDER } from "#lib/files/filters.js";
  import { plural } from "#lib/format.js";
  import { type ActiveFilter, TEXT_FILTER } from "#lib/lists/filtered.js";
  import { HIDING } from "#lib/search/results.js";
  import { type ResultSort, searchOptions } from "#lib/settings.svelte.js";

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
    /** the Fold toggle changed: every card folds or unfolds */
    onfold: (fold: boolean) => void;
  };

  let {
    filter = $bindable(),
    counts,
    filters,
    running,
    sortable,
    blockedCount,
    showBlocked = $bindable(),
    onfold
  }: Props = $props();

  const SORTS: { value: ResultSort; label: string }[] = [
    { value: "uploadSpeed", label: "Fastest upload speed" },
    { value: "queueLength", label: "Shortest queue" }
  ];

  /** the filters behind the phone's Filters button: the chips', not the text box's */
  const toggles = $derived(filters.filter((f) => f.id !== TEXT_FILTER));

  const VIEWS = [
    { id: "cards", label: "Tree", icon: FolderTreeIcon },
    { id: "dense", label: "Dense", icon: Rows3Icon }
  ] as const;
</script>

<div class="flex flex-col gap-2.5 rounded-card border bg-surface p-3 shadow-card">
  <div class="flex flex-wrap items-center gap-x-5 gap-y-2">
    <Segmented options={VIEWS} bind:value={searchOptions.view} label="View" size="sm" />
    {#if searchOptions.view === "cards"}
      <div class="flex max-w-full min-w-0 items-center gap-2">
        <ArrowUpDownIcon class="size-4 shrink-0 text-fg-muted" />
        <Select
          bind:value={searchOptions.sort}
          options={SORTS}
          aria-label="Sort"
          class="h-control-sm w-64 min-w-0"
          disabled={!sortable} />
      </div>
    {/if}
    <InspectorToggle />
    <p class="ml-auto flex items-center gap-1.5 text-sm text-fg-muted tabular-nums" role="status">
      {#if running}<Spinner class="size-3.5" />{/if}
      <!-- with a filter on, the line of filters under the toolbar carries the count -->
      {#if !filters.length}
        {plural(counts.files, "file", "files")} from {plural(counts.users, "user", "users")}
        {#if counts.users !== counts.allUsers}
          <span class="text-fg-subtle">
            (of {counts.allFiles.toLocaleString()} from {counts.allUsers.toLocaleString()})
          </span>
        {/if}
      {/if}
    </p>
  </div>
  <div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
    <Filter.Panel filters={toggles}>
      <Filter.Group label="Hide">
        {#each HIDING as h (h.key)}
          <Filter.Chip
            pressed={searchOptions[h.key]}
            title={h.hint}
            onclick={() => (searchOptions[h.key] = !searchOptions[h.key])}>{h.chip}</Filter.Chip>
        {/each}
      </Filter.Group>
    </Filter.Panel>
    {#if searchOptions.view === "cards"}
      <label class="flex items-center gap-2">
        <Switch bind:checked={searchOptions.fold} onCheckedChange={(fold) => onfold(fold)} />Fold
        results
      </label>
    {/if}
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
