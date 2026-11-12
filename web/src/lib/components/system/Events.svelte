<script lang="ts" module>
  import type { PageSizes } from "#lib/lists/pages.js";
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the page sizes the event log offers, and the one it starts at */
  export const EVENT_PAGES: PageSizes = { sizes: [10, 25, 50, 100], size: 25 };

  /** how long the first page waits after a raised event, so a burst of them is read once */
  export const RAISED_SETTLE_MS = 250;

  /** the event log's column widths, kept in this browser; the daemon orders it, newest first */
  const view = gridView("reseek.system.events", [], null);
</script>

<script lang="ts">
  import { goto } from "$app/navigation";
  import CalendarCheckIcon from "@lucide/svelte/icons/calendar-check";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import InfoIcon from "@lucide/svelte/icons/info";
  import Maximize2Icon from "@lucide/svelte/icons/maximize-2";
  import { untrack } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Pagination from "#lib/components/ui/Pagination.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { dateTime, plural } from "#lib/format.js";
  import type { ActiveFilter } from "#lib/lists/filtered.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { pageCount, type Place, withPlace } from "#lib/lists/pages.js";
  import { Grid, keptIn } from "#lib/lists/table.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { reason, UNREACHABLE } from "#lib/request.js";
  import { EVENT_TYPES, eventSummary, payloadText } from "#lib/system/events.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  // the event log, newest first, a page at a time, with its type filter and page size in the URL
  type Props = { place: Place; types: string[] };
  let { place, types }: Props = $props();
  const page = $derived(place.page);
  const perPage = $derived(place.size);

  type Record = Schemas["EventRecord"];

  const rows = new Keyed<Record>((e) => e.id);
  let shown = $state<Record | null>(null);

  /** the page the props ask for, with how many events match in all */
  const listed = new Loader<{ events: Record[]; total: number }>(
    async () => {
      const query = {
        offset: (page - 1) * perPage,
        limit: perPage,
        type: types.length ? types.join(",") : undefined
      };
      let reply;
      try {
        reply = await api.GET("/api/v0/events", { params: { query } });
      } catch {
        return { ok: false, error: UNREACHABLE, status: 0 };
      }
      const status = reply.response.status;
      if (!reply.response.ok || !reply.data) {
        return { ok: false, error: reason(status, reply.error), status };
      }
      const total = Number(reply.response.headers.get("X-Total-Count")) || 0;
      return { ok: true, data: { events: reply.data, total } };
    },
    (data) => rows.reconcile(data.events)
  );
  keepCurrent(
    () => void listed.load(),
    () => "events"
  );
  $effect(() => {
    void page;
    void perPage;
    void types;
    untrack(() => void listed.load());
  });
  // the first page follows new events as they are raised, read once for a burst of them
  $effect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stop = live.on("events.raised", () => {
      if (page !== 1 || timer !== undefined) return;
      timer = setTimeout(() => {
        timer = undefined;
        void listed.load();
      }, RAISED_SETTLE_MS);
    });
    return () => {
      stop();
      clearTimeout(timer);
    };
  });

  const events = $derived((listed.data?.events ?? []).flatMap((e) => rows.get(e.id) ?? []));
  const grid = new Grid<Record>({
    columns: () => [
      { id: "id", label: "Id", width: "2.5rem", quiet: true, fixed: true },
      { id: "timestamp", label: "Timestamp", width: "11rem" },
      { id: "type", label: "Type", width: "13.5rem" },
      { id: "data", label: "Data", width: "16rem", grow: true }
    ],
    data: () => events,
    ...keptIn(view),
    // a phone puts the type and the time above the data, in its one column
    shown: () => (viewport.phone ? ["data"] : ["id", "timestamp", "type", "data"])
  });
  const pages = $derived(pageCount(listed.data?.total ?? 0, perPage));
  const list = new ListControl<Record>({
    lines: () => events,
    key: (e) => e.id,
    id: (e) => e.id,
    onopen: (e) => (shown = e)
  });

  /** Writes the page, its size and the types into the address, which the props follow */
  const to = (next: Partial<{ page: number; perPage: number; types: string[] }>) => {
    const p = { page, perPage, types, ...next };
    const q = withPlace(
      new URLSearchParams(p.types.length ? { type: p.types.join(",") } : {}),
      { page: p.page, size: p.perPage },
      EVENT_PAGES
    );
    void goto(href(`/system/events${q.size ? `?${q}` : ""}`), { replace: true });
  };
  const toggle = (type: string) =>
    to({
      page: 1,
      types: types.includes(type) ? types.filter((t) => t !== type) : [...types, type]
    });

  const EVENTS = ["event", "events"] as const;
  let toolbar = $state<HTMLElement | null>(null);
  /** the list reloads as the types change, so focus goes to the types menu, which stays */
  const refocus = () => toolbar?.querySelector("button")?.focus();
  const filters = $derived<ActiveFilter[]>(
    types.map((type) => ({
      id: type,
      label: type,
      clear: () => to({ page: 1, types: types.filter((t) => t !== type) })
    }))
  );
  // the types are the URL's, which a type's own clear writes from as they were, so clearing them
  // one after another would keep all but the last
  const clearTypes = () => to({ page: 1, types: [] });
</script>

<div class="flex flex-wrap items-center gap-inline" bind:this={toolbar}>
  <Menu.Root>
    <Menu.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="outline" size="sm" class={[types.length && Filter.FILTERING]}
          >{types.length
            ? plural(types.length, "type", "types")
            : "All types"}<ChevronDownIcon /></Button>
      {/snippet}
    </Menu.Trigger>
    <Menu.Content align="start" class="max-h-80 overflow-y-auto">
      {#each EVENT_TYPES as type (type)}
        <Menu.CheckboxItem
          closeOnSelect={false}
          checked={types.includes(type)}
          onCheckedChange={() => toggle(type)}>{type}</Menu.CheckboxItem>
      {/each}
    </Menu.Content>
  </Menu.Root>
  <div class="ml-auto flex flex-wrap items-center justify-end gap-inline">
    <Select
      class="h-control-sm w-32"
      aria-label="Events per page"
      bind:value={() => perPage, (v) => to({ page: 1, perPage: v })}
      options={EVENT_PAGES.sizes.map((n) => ({ value: n, label: `${n} per page` }))} />
    <Pagination {page} {pages} onpage={(p) => to({ page: p })} />
  </div>
</div>

{#if listed.data}
  <Filter.Summary
    {filters}
    shown={listed.data.total}
    total={null}
    noun={EVENTS}
    {refocus}
    onclear={clearTypes} />
{/if}

{#if listed.error && !events.length}
  <ErrorSegment caption={listed.error} retry={() => void listed.load()} retryLabel="Retry" />
{:else if listed.loading}
  <Skeleton class="h-64" />
{:else if !events.length && types.length}
  <Filter.Empty {filters} total={null} noun={EVENTS} {refocus} onclear={clearTypes} />
{:else if !events.length}
  <EmptyState icon={CalendarCheckIcon} title="No events" />
{:else}
  <div
    class="overflow-x-auto rounded-card border bg-surface text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Events"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed}>
    <GridHeader {grid} class="h-8 border-b bg-surface-muted text-fg-muted" />
    {#each events as e, i (e.id)}
      {@const summary = eventSummary(e)}
      <div
        role="row"
        {...list.line(e, i)}
        class="grid items-center list-row py-1 font-mono text-xs"
        style={grid.row}>
        {#if !viewport.phone}
          <span role="gridcell" class="flex px-2">
            <Tooltip text={e.id} dense>
              {#snippet child({ props })}
                <InfoIcon {...props} class="size-4 text-fg-muted" aria-label={e.id} />
              {/snippet}
            </Tooltip>
          </span>
          <Tooltip text={e.timestamp} dense>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate px-2">{dateTime(e.timestamp)}</span>
            {/snippet}
          </Tooltip>
          <Tooltip text={e.type} clipped>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate px-2">{e.type}</span>
            {/snippet}
          </Tooltip>
        {/if}
        <span role="gridcell" class="flex min-w-0 items-center gap-2 pr-1 pl-2">
          <span class="flex min-w-0 flex-1 flex-col">
            {#if viewport.phone}
              <span class="flex min-w-0 gap-2">
                <span class="truncate">{e.type}</span>
                <span class="ml-auto shrink-0 text-fg-muted">{dateTime(e.timestamp)}</span>
              </span>
            {/if}
            <span class="flex min-w-0 items-center gap-2">
              {#if summary.user}<span class="flex max-w-1/2 min-w-0 shrink-0 font-sans"
                  ><Username name={summary.user} /></span
                >{/if}
              <Tooltip text={summary.text} clipped>
                {#snippet child({ props })}
                  <span
                    {...props}
                    class="min-w-0 flex-1 truncate font-sans text-fg-muted"
                    dir="auto">{summary.text}</span>
                {/snippet}
              </Tooltip>
            </span>
          </span>
          <Tooltip text="Show full payload">
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                class="shrink-0"
                aria-label="Show full payload"
                onclick={() => (shown = e)}><Maximize2Icon /></Button>
            {/snippet}
          </Tooltip>
        </span>
      </div>
    {/each}
  </div>
{/if}

<Dialog
  bind:open={() => shown !== null, (o) => !o && (shown = null)}
  title={shown?.type ?? ""}
  description={shown ? `${shown.id} at ${shown.timestamp}` : undefined}
  class="w-[min(40rem,calc(100vw-2rem))]">
  <pre
    class="max-h-[60svh] overflow-auto rounded-control bg-surface-muted p-3 font-mono text-xs wrap-anywhere whitespace-pre-wrap">{shown
      ? payloadText(shown.data)
      : ""}</pre>
</Dialog>
