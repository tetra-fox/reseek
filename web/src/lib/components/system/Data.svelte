<script lang="ts">
  import DatabaseIcon from "@lucide/svelte/icons/database";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";

  import { api, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { sections, valueAt } from "#lib/config/sections.js";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { confirmation } from "#lib/feedback/confirm.svelte.js";
  import { formatBytes, parseDuration, sayDuration } from "#lib/format.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";

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
      body: "They stay in the statistics",
      confirm: "Clear",
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

  /** The retention options as the settings page names and orders them, with their values */
  const retention = $derived(
    sections
      .find((s) => s.id === "retention")!
      .fields.map((f) => ({ key: f.key, label: f.label, value: valueAt(app.options, f.key) }))
  );
  /** a retention option's value as people say it: an age, a count, or kept forever */
  const retentionText = (value: unknown) => {
    if (value === null || value === undefined) return "Kept forever";
    if (typeof value === "number") return value.toLocaleString();
    if (typeof value === "boolean") return value ? "Yes" : "No";
    const ms = parseDuration(value);
    return ms === null ? String(value) : sayDuration(ms);
  };
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
      description="reseek keeps its databases in memory" />
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
            // a phone puts the path on a line of its own and leaves out the log's size
            "grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-x-2 gap-y-0.5 list-row px-5 py-1.5 md:grid-cols-[10rem_minmax(0,1fr)_6rem_6rem_6rem]"
          ]}>
          <span role="gridcell" class="truncate font-medium">{d.name}</span>
          <span
            role="gridcell"
            class="truncate font-mono text-xs text-fg-muted max-md:col-span-3 max-md:row-start-2"
            title={d.path}>{d.path}</span>
          <span role="gridcell" class="text-right tabular-nums">{formatBytes(d.sizeBytes)}</span>
          <span role="gridcell" class="text-right text-xs text-fg-muted tabular-nums max-md:hidden"
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
  <dl
    class="grid grid-cols-[minmax(0,1fr)] gap-x-6 gap-y-1 text-sm md:grid-cols-[auto_minmax(0,1fr)]">
    {#each retention as r (r.key)}
      <dt class="text-fg-muted" title={r.key}>{r.label}</dt>
      <dd class="max-md:mb-1">{retentionText(r.value)}</dd>
    {/each}
  </dl>
  <a href={href("/system/settings")} class="text-sm text-accent hover:underline"
    >Edit in Settings</a>
</Card>

<Card title="Finished transfers">
  <p class="text-sm text-fg-muted">
    Long lists slow down Downloads and Uploads. Cleared transfers stay in the statistics.
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
