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
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Pagination from "#lib/components/ui/Pagination.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { dateTime, plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { reason, UNREACHABLE } from "#lib/request.js";
  import { EVENT_TYPES } from "#lib/system/events.js";

  // the event log, newest first, a page at a time, with its type filter and page size in the URL
  type Props = { page: number; perPage: number; types: string[] };
  let { page, perPage, types }: Props = $props();

  type Record = Schemas["EventRecord"];
  const SIZES = [10, 25, 50, 100];

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
  // the first page follows new events as they are raised
  $effect(() =>
    live.on("events.raised", () => {
      if (page === 1) void listed.load();
    })
  );

  const events = $derived((listed.data?.events ?? []).flatMap((e) => rows.get(e.id) ?? []));
  const pages = $derived(Math.max(1, Math.ceil((listed.data?.total ?? 0) / perPage)));
  const list = new ListControl<Record>({
    lines: () => events,
    key: (e) => e.id,
    id: (e) => e.id,
    onopen: (e) => (shown = e)
  });

  const to = (next: Partial<Props>) => {
    const p = { page, perPage, types, ...next };
    const q = new URLSearchParams({ page: String(p.page), per_page: String(p.perPage) });
    if (p.types.length) q.set("type", p.types.join(","));
    void goto(href(`/system/events?${q}`), { replaceState: true });
  };
  const toggle = (type: string) =>
    to({
      page: 1,
      types: types.includes(type) ? types.filter((t) => t !== type) : [...types, type]
    });

  const pretty = (e: Record) => JSON.stringify(e.data, null, 2);
  const userOf = (e: Record) => {
    const data = e.data as { username?: unknown };
    return typeof data.username === "string" ? data.username : null;
  };
</script>

<div class="flex flex-wrap items-center gap-inline">
  <Menu.Root>
    <Menu.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="outline" size="sm"
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
  {#if types.length}
    <Button variant="ghost" size="sm" onclick={() => to({ page: 1, types: [] })}>All types</Button>
  {/if}
  <div class="ml-auto flex flex-wrap items-center justify-end gap-inline">
    <Select
      class="h-control-sm w-32"
      aria-label="Events per page"
      bind:value={() => perPage, (v) => to({ page: 1, perPage: v })}
      options={SIZES.map((n) => ({ value: n, label: `${n} per page` }))} />
    <Pagination {page} {pages} onpage={(p) => to({ page: p })} />
  </div>
</div>

{#if listed.error && !events.length}
  <ErrorSegment caption={listed.error} retry={() => void listed.load()} retryLabel="Retry" />
{:else if listed.loading}
  <Skeleton class="h-64" />
{:else if !events.length}
  <EmptyState icon={CalendarCheckIcon} title="No events" />
{:else}
  <div
    class="overflow-x-auto rounded-card border bg-surface text-sm shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Events"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed}>
    <div
      role="row"
      class="grid min-w-max grid-cols-[2rem_9rem_13rem_minmax(16rem,1fr)] gap-2 border-b bg-surface-muted px-2 py-1.5 text-xs font-medium text-fg-muted">
      <span role="columnheader">Id</span>
      <span role="columnheader">Timestamp</span>
      <span role="columnheader">Type</span>
      <span role="columnheader">Data</span>
    </div>
    {#each events as e, i (e.id)}
      {@const user = userOf(e)}
      <div
        role="row"
        {...list.line(e, i)}
        class="grid min-w-max grid-cols-[2rem_9rem_13rem_minmax(16rem,1fr)] items-center gap-2 list-row px-2 py-1 font-mono text-xs">
        <span role="gridcell">
          <Tooltip text={e.id}>
            {#snippet child({ props })}
              <InfoIcon {...props} class="size-4 text-fg-muted" aria-label={e.id} />
            {/snippet}
          </Tooltip>
        </span>
        <span role="gridcell" title={e.timestamp}>{dateTime(e.timestamp)}</span>
        <span role="gridcell" class="truncate">{e.type}</span>
        <span role="gridcell" class="flex min-w-0 items-center gap-2">
          {#if user}<span class="font-sans"><Username name={user} /></span>{/if}
          <span class="min-w-0 flex-1 truncate text-fg-muted">{JSON.stringify(e.data)}</span>
          <Button
            variant="ghost"
            size="icon-sm"
            class="shrink-0"
            aria-label="Show full payload"
            title="Show full payload"
            onclick={() => (shown = e)}><Maximize2Icon /></Button>
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
    class="max-h-[60svh] overflow-auto rounded-control bg-surface-muted p-3 font-mono text-xs">{shown
      ? pretty(shown)
      : ""}</pre>
</Dialog>
