<script lang="ts">
  import DatabaseIcon from "@lucide/svelte/icons/database";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";

  import { api, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { Busy } from "#lib/busy.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { confirmation } from "#lib/confirm.svelte.js";
  import { formatBytes } from "#lib/format.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";

  // the databases on disk with their sizes, the retention in force, and the transfer lists' upkeep
  const databases = new Loader(() => call(() => api.GET("/api/v0/application/data")));
  keepCurrent(() => void databases.load());
  void databases.load();

  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));
  const busy = new Busy();
  const rows = $derived(databases.data ?? []);

  const list = new ListControl<Schemas["DatabaseFile"]>({
    lines: () => rows,
    key: (d) => d.name,
    id: (d) => d.name
  });

  const vacuum = (names: string[]) =>
    void busy.run(
      async () => {
        let last: Awaited<ReturnType<typeof call>> = { ok: true, data: undefined };
        for (const name of names) {
          last = await call(() =>
            api.POST("/api/v0/application/data/{name}/vacuum", { params: { path: { name } } })
          );
          if (!last.ok) break;
        }
        void databases.load();
        return last;
      },
      names.length === 1 ? `${names[0]} vacuumed` : `${names.length} databases vacuumed`
    );

  const clearCompleted = (direction: "uploads" | "downloads") =>
    confirmation.ask({
      title: `Clear finished ${direction}?`,
      body: `All finished ${direction} leave the list. They still count in the statistics.`,
      confirm: "Remove",
      run: () =>
        void busy.run(
          () =>
            call(() =>
              direction === "uploads"
                ? api.DELETE("/api/v0/transfers/uploads/all/completed")
                : api.DELETE("/api/v0/transfers/downloads/all/completed")
            ),
          `Finished ${direction} cleared`
        )
    });

  /** The retention options as dotted keys and their values, for a table */
  const flatten = (value: unknown, prefix: string): [string, unknown][] =>
    value && typeof value === "object" && !Array.isArray(value)
      ? Object.entries(value).flatMap(([k, v]) => flatten(v, `${prefix}.${k}`))
      : [[prefix, value]];
  const retention = $derived(flatten(fields(app.options).retention, "retention"));
  const chosen = $derived(rows.filter((d) => list.selected.has(d.name)).map((d) => d.name));
</script>

<Card title="Databases">
  {#snippet action()}
    {#if admin && chosen.length}
      <Button size="sm" variant="outline" disabled={busy.running} onclick={() => vacuum(chosen)}
        >Vacuum selected</Button>
    {/if}
  {/snippet}
  <LoadError loader={databases} />
  {#if databases.loading}
    <Skeleton class="h-24" />
  {:else if !rows.length}
    <EmptyState
      icon={DatabaseIcon}
      title="No database files"
      description="reseek keeps its databases in memory." />
  {:else}
    <div
      class="-mx-5 border-y text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
      role="grid"
      aria-label="Databases"
      aria-multiselectable="true"
      aria-activedescendant={list.activeId}
      tabindex="0"
      {@attach list.attach}>
      {#each rows as d, i (d.name)}
        <div
          role="row"
          {...list.line(d, i)}
          class={[
            "grid grid-cols-[10rem_1fr_6rem_6rem_6rem] items-center gap-2 border-b px-5 py-1.5 last:border-b-0 hover:bg-hover",
            list.selected.has(d.name) && "bg-accent-soft/60",
            list.isFocused(d) && "ring-1 ring-accent ring-inset"
          ]}>
          <span role="gridcell" class="font-medium">{d.name}</span>
          <span role="gridcell" class="truncate font-mono text-xs text-fg-muted" title={d.path}
            >{d.path}</span>
          <span role="gridcell" class="text-right tabular-nums">{formatBytes(d.sizeBytes)}</span>
          <span role="gridcell" class="text-right text-xs text-fg-muted tabular-nums"
            >{d.walSizeBytes ? `+${formatBytes(d.walSizeBytes)} log` : ""}</span>
          <span role="gridcell" class="flex justify-end">
            {#if admin}
              <Button
                size="sm"
                variant="ghost"
                disabled={busy.running}
                onclick={() => vacuum([d.name])}>Vacuum</Button>
            {/if}
          </span>
        </div>
      {/each}
    </div>
  {/if}
</Card>

<Card title="Retention">
  <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm">
    {#each retention as [key, value] (key)}
      <dt class="font-mono text-xs text-fg-muted">{key}</dt>
      <dd>{value === null || value === undefined ? "Kept forever" : String(value)}</dd>
    {/each}
  </dl>
  <a href={href("/system/settings")} class="text-sm text-accent hover:underline"
    >Change them in Settings</a>
</Card>

<Card title="Finished transfers">
  <p class="text-sm text-fg-muted">
    Very long lists can slow down the Downloads and Uploads pages. Clearing finished transfers keeps
    them in the statistics.
  </p>
  {#if write}
    <div class="flex flex-wrap gap-inline">
      <Button disabled={busy.running} onclick={() => clearCompleted("uploads")}
        ><Trash2Icon />Clear finished uploads</Button>
      <Button disabled={busy.running} onclick={() => clearCompleted("downloads")}
        ><Trash2Icon />Clear finished downloads</Button>
    </div>
  {/if}
</Card>
