<script lang="ts">
  import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
  import FilterIcon from "@lucide/svelte/icons/filter";
  import LayoutListIcon from "@lucide/svelte/icons/layout-list";
  import Rows3Icon from "@lucide/svelte/icons/rows-3";

  import Input from "#lib/components/ui/Input.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import { plural } from "#lib/format.js";
  import { FILTER_PLACEHOLDER } from "#lib/search/filters.js";
  import { LONG_QUEUE, type ResultSort, searchOptions } from "#lib/settings.svelte.js";

  import FilterHelp from "./FilterHelp.svelte";
  import FilterPresets from "./FilterPresets.svelte";

  type Props = {
    filter: string;
    /** what the filter and toggles leave, out of everything that came */
    counts: { files: number; users: number; allFiles: number; allUsers: number };
    /** results are still coming */
    running: boolean;
    /** the results are all in, so sorting them moves nothing under the reader */
    sortable: boolean;
    /** how many users are blocked, whose results show only when asked */
    blockedCount: number;
    showBlocked: boolean;
    /** the Fold toggle changed: every card folds or unfolds */
    onfold: (fold: boolean) => void;
    /** the toolbar's height, which what stays in view under it starts below */
    height?: number;
  };

  let {
    filter = $bindable(),
    counts,
    running,
    sortable,
    blockedCount,
    showBlocked = $bindable(),
    onfold,
    height = $bindable(0)
  }: Props = $props();

  const SORTS: { value: ResultSort; label: string }[] = [
    { value: "uploadSpeed", label: "Fastest upload speed" },
    { value: "queueLength", label: "Shortest queue" }
  ];

  const VIEWS = [
    { id: "cards", label: "Cards", icon: LayoutListIcon },
    { id: "dense", label: "Dense", icon: Rows3Icon }
  ] as const;
</script>

<!-- on a wide screen it stays in view over the results, above the dense view's own header, so
     the filter is always at hand; a phone can't spare the height, so there it scrolls away -->
<div
  class="flex flex-col gap-2.5 rounded-card border bg-surface/95 p-3 shadow-card backdrop-blur-md md:sticky md:top-0 md:z-20"
  bind:offsetHeight={height}>
  <div class="flex flex-wrap items-center gap-x-5 gap-y-2">
    <div class="inline-flex rounded-control border p-0.5" role="radiogroup" aria-label="View">
      {#each VIEWS as v (v.id)}
        <button
          type="button"
          role="radio"
          aria-checked={searchOptions.view === v.id}
          class={[
            "flex items-center gap-1.5 rounded-badge px-2 py-0.5 text-sm",
            searchOptions.view === v.id ? "bg-surface-muted font-medium" : "text-fg-muted"
          ]}
          onclick={() => (searchOptions.view = v.id)}>
          <v.icon class="size-3.5" />{v.label}
        </button>
      {/each}
    </div>
    {#if searchOptions.view === "cards"}
      <div class="flex max-w-full min-w-0 items-center gap-2">
        <ArrowUpDownIcon class="size-4 shrink-0 text-fg-muted" />
        <Select
          bind:value={searchOptions.sort}
          options={SORTS}
          aria-label="Sort"
          class="w-64 min-w-0"
          disabled={!sortable} />
      </div>
    {/if}
    <p class="ml-auto flex items-center gap-1.5 text-sm text-fg-muted tabular-nums" role="status">
      {#if running}<Spinner class="size-3.5" />{/if}
      {plural(counts.files, "file", "files")} from {plural(counts.users, "user", "users")}
      {#if counts.users !== counts.allUsers}
        <span class="text-fg-subtle">
          (of {counts.allFiles.toLocaleString()} from {counts.allUsers.toLocaleString()})
        </span>
      {/if}
    </p>
  </div>
  <div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
    <label class="flex items-center gap-2">
      <Switch bind:checked={searchOptions.hideLocked} />Hide locked files
    </label>
    <label class="flex items-center gap-2">
      <Switch bind:checked={searchOptions.hideNoFreeSlots} />Hide users with no free slot
    </label>
    <label class="flex items-center gap-2" title="Users who were offline when last seen">
      <Switch bind:checked={searchOptions.hideOffline} />Hide offline users
    </label>
    <label class="flex items-center gap-2">
      <Switch bind:checked={searchOptions.hideLongQueues} />Hide queues longer than {LONG_QUEUE}
    </label>
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
      class="font-mono text-xs md:text-xs" />
    <FilterHelp />
  </div>
  <FilterPresets bind:filter />
</div>
