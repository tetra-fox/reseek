<script lang="ts" module>
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the shares table's sort and column widths, kept in this browser */
  const sharesView = gridView(
    "reseek.system.shares",
    ["host", "path", "folders", "files", "alias", "remote", "visible"],
    null
  );
</script>

<script lang="ts">
  import CopyIcon from "@lucide/svelte/icons/copy";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import XIcon from "@lucide/svelte/icons/x";
  import { untrack } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Progress from "#lib/components/ui/Progress.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { editability, fileOf } from "#lib/config/editability.js";
  import { valueAt } from "#lib/config/sections.js";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { columnWidth, plural, WIDEST } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { Grid, keptIn } from "#lib/lists/table.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { contentsText, VISIBILITY, withVisibility } from "#lib/system/shares.js";

  import AddShare from "./AddShare.svelte";

  // the shared directories by host, the exclusions, and the scan that counts them
  type Row = Schemas["Share"] & { host: string };

  const rows = new Keyed<Row>((r) => `${r.host}\u0000${r.localPath}`);
  const shares = new Loader(async () => {
    const outcome = await call(() => api.GET("/api/v0/shares"));
    if (outcome.ok) {
      rows.reconcile(
        Object.entries(outcome.data).flatMap(([host, list]) => list.map((s) => ({ ...s, host })))
      );
    }
    return outcome;
  });
  keepCurrent(() => void shares.load());
  void shares.load();

  const scan = $derived(app.state?.shares);
  // a relay agent's shares come and go with it
  const hosts = $derived(scan?.hosts?.join("\u0000") ?? "");
  // a scan that starts or ends changes the counts; they are written just after the state flips
  $effect(() => {
    void hosts;
    const pending = scan?.scanPending;
    const scanning = scan?.scanning;
    if (pending === undefined) return;
    untrack(() => void shares.load());
    if (scanning) return;
    const timer = setTimeout(() => void shares.load(), 1_000);
    return () => clearTimeout(timer);
  });

  const grid = new Grid<Row>({
    columns: () => [
      { id: "host", label: "Host", width: "6rem", value: (r) => r.host.toLowerCase() },
      {
        id: "path",
        label: "Local path",
        width: "12rem",
        grow: true,
        value: (r) => r.localPath.toLowerCase()
      },
      {
        id: "folders",
        label: "Folders",
        width: columnWidth(WIDEST.count),
        end: true,
        descFirst: true,
        value: (r) => r.directories ?? -1
      },
      {
        id: "files",
        label: "Files",
        width: columnWidth(WIDEST.count),
        end: true,
        descFirst: true,
        value: (r) => r.files ?? -1
      },
      {
        id: "alias",
        label: "Alias",
        width: "6rem",
        grow: true,
        value: (r) => r.alias.toLowerCase()
      },
      {
        id: "remote",
        label: "Remote path",
        width: "6rem",
        grow: true,
        value: (r) => r.remotePath.toLowerCase()
      },
      { id: "visible", label: "Visible to", width: "10rem", value: (r) => VISIBILITY[r.visibility] }
    ],
    data: () => rows.rows.filter((r) => !r.isExcluded),
    ...keptIn(sharesView),
    clearable: true,
    ties: "path"
  });
  const shared = $derived(grid.rows);
  const excluded = $derived(rows.rows.filter((r) => r.isExcluded));
  const write = $derived(session.can("ReadWrite"));
  const busy = new Busy();
  let open = $state<Row | null>(null);

  const key = (r: Row) => `${r.host}\u0000${r.localPath}`;
  const sharedList = new ListControl<Row>({
    lines: () => shared,
    key,
    id: key,
    onopen: (r) => (open = r)
  });
  const excludedList = new ListControl<Row>({ lines: () => excluded, key, id: key });

  let contents = $state<string | null>(null);
  let contentsError = $state("");
  // each share opened reads its contents
  $effect(() => {
    const share = open;
    if (!share) return;
    contents = null;
    contentsError = "";
    untrack(async () => {
      const outcome = await call(() =>
        api.GET("/api/v0/shares/{id}/contents", { params: { path: { id: share.id } } })
      );
      if (open !== share) return;
      if (outcome.ok) contents = contentsText(share, outcome.data);
      else contentsError = outcome.error;
    });
  });

  // who sees each share is two lists of aliases in the options, changed here where the options
  // can be saved to their file
  const BUDDIES_KEY = "shares.buddies_only";
  const TRUSTED_KEY = "shares.trusted_only";
  const admin = $derived(session.can("Administrator"));
  const view = new Loader(() => call(() => api.GET("/api/v0/options/view")));
  $effect(() => {
    if (admin) untrack(() => void view.load());
  });
  $effect(() =>
    live.on("application.options", () => {
      if (admin) void view.load();
      void shares.load();
    })
  );
  const settable = $derived.by(() => {
    const data = view.data;
    if (!data || valueAt(data.values, "remote_configuration") !== true) return false;
    const file = fileOf(data, BUDDIES_KEY);
    return (
      file === fileOf(data, TRUSTED_KEY) &&
      [BUDDIES_KEY, TRUSTED_KEY].every((key) => editability(data, key, "page").mode === "save")
    );
  });
  const VISIBILITIES = (["everyone", "buddies", "trusted"] as const).map((value) => ({
    value,
    label: VISIBILITY[value]
  }));
  const setVisibility = async (r: Row, to: Schemas["ShareVisibility"]) => {
    const data = view.data;
    if (!data || to === r.visibility) return;
    const lists = withVisibility(
      {
        buddies: (valueAt(data.values, BUDDIES_KEY) ?? []) as string[],
        trusted: (valueAt(data.values, TRUSTED_KEY) ?? []) as string[]
      },
      r.alias,
      to
    );
    const file = fileOf(data, BUDDIES_KEY);
    const outcome = await call(() =>
      api.PATCH("/api/v0/options/file", {
        body: {
          file,
          revision: data.files.find((f) => f.name === file)?.revision ?? null,
          set: { [BUDDIES_KEY]: lists.buddies, [TRUSTED_KEY]: lists.trusted },
          unset: []
        }
      })
    );
    if (!outcome.ok) {
      toast(outcome.error, "error");
      // a file changed on disk is read again, so a second try goes through
      return void view.load();
    }
    // every share on any host with the alias, as the daemon matches it
    for (const row of rows.rows)
      if (row.alias.toLowerCase() === r.alias.toLowerCase())
        rows.upsert({ ...row, visibility: to });
    void view.load();
  };
</script>

<div class="flex flex-wrap items-center gap-inline">
  {#if write && scan}
    {#if scan.scanning}
      <Button
        variant="danger"
        disabled={busy.running}
        onclick={() => void busy.run(() => call(() => api.DELETE("/api/v0/shares")))}
        ><XIcon />Cancel scan</Button>
    {:else}
      <Button
        variant={scan.scanPending ? "secondary" : "primary"}
        class={scan.scanPending ? "text-warning" : undefined}
        disabled={busy.running}
        onclick={() =>
          void busy.run(() => call(() => api.PUT("/api/v0/shares", { params: { query: {} } })))}>
        {#if busy.running}<Spinner class="size-4" />{:else}<RefreshCwIcon />{/if}Rescan now
      </Button>
    {/if}
  {/if}
  <AddShare onadded={() => void shares.load()} />
</div>

{#if scan?.faulted}
  <Notice kind="warning">The last scan failed. See Logs for details.</Notice>
{:else if scan?.cancelled}
  <Notice kind="warning">The last scan was canceled. Shares may be incomplete.</Notice>
{/if}
<LoadError loader={shares} />

{#if scan?.scanning}
  {@const percent = Math.round(scan.scanProgress * 100)}
  <div
    class="flex flex-col gap-2 rounded-card border bg-surface p-4"
    role="status"
    aria-live="polite">
    <div class="flex items-baseline gap-3">
      <span class="text-sm font-medium">Scanning shares</span>
      <span class="ml-auto text-sm font-medium tabular-nums">{percent}%</span>
    </div>
    <Progress value={percent} label="Scan progress" />
    <span class="text-xs text-fg-muted tabular-nums"
      >Found {plural(scan.files, "file", "files")} in {plural(
        scan.directories,
        "folder",
        "folders"
      )} so far</span>
  </div>
{/if}
{#if shares.loading && !rows.size}
  <Skeleton class="h-40" />
{:else}
  <Card title="Shares" class={scan?.scanning ? "opacity-60" : undefined}>
    {#snippet flush()}
      <div
        class="overflow-x-auto border-t text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
        role="grid"
        aria-label="Shared folders"
        aria-multiselectable="true"
        aria-activedescendant={sharedList.activeId}
        tabindex="0"
        {@attach sharedList.attach}
        {@attach freezesWhileUsed}>
        <GridHeader {grid} class="h-8 border-b bg-surface-muted text-fg-muted" />
        {#each shared as r, i (key(r))}
          <div
            role="row"
            {...sharedList.line(r, i)}
            class={["grid items-center list-row py-1.5", r.derivedFrom && "text-fg-muted"]}
            style={grid.row}>
            <span role="gridcell" class="truncate px-2">{r.host}</span>
            <span role="gridcell" class="min-w-0 px-2">
              <button
                type="button"
                class="flex max-w-full items-center gap-1.5 text-left text-accent hover:underline"
                onclick={() => (open = r)}>
                <FolderIcon class="size-4 shrink-0" />
                <Tooltip text={r.localPath} clipped>
                  {#snippet child({ props })}
                    <span {...props} class="truncate font-mono text-xs">{r.localPath}</span>
                  {/snippet}
                </Tooltip>
              </button>
              {#if r.derivedFrom}
                <span class="text-xs">
                  Shared because
                  <a href={href("/system/settings")} class="underline"
                    >Share the downloads folder</a>
                  is on (<code>{r.derivedFrom}</code>)
                </span>
              {/if}
            </span>
            <span role="gridcell" class="truncate px-2 text-right tabular-nums"
              >{r.directories?.toLocaleString() ?? "?"}</span>
            <span role="gridcell" class="truncate px-2 text-right tabular-nums"
              >{r.files?.toLocaleString() ?? "?"}</span>
            <Tooltip text={r.alias} clipped>
              {#snippet child({ props })}
                <span {...props} role="gridcell" class="truncate px-2">{r.alias}</span>
              {/snippet}
            </Tooltip>
            <Tooltip text={r.remotePath} clipped>
              {#snippet child({ props })}
                <span {...props} role="gridcell" class="truncate px-2 font-mono text-xs"
                  >{r.remotePath}</span>
              {/snippet}
            </Tooltip>
            <span role="gridcell" class="px-2">
              {#if settable}
                <Select
                  class="h-control-sm w-full"
                  aria-label={`Who sees ${r.alias}`}
                  bind:value={() => r.visibility, (to) => void setVisibility(r, to)}
                  options={VISIBILITIES} />
              {:else}
                {VISIBILITY[r.visibility]}
              {/if}
            </span>
          </div>
        {:else}
          <EmptyRow text="No shares" />
        {/each}
      </div>
    {/snippet}
  </Card>

  <Card title="Excluded paths">
    {#snippet flush()}
      <div
        class="border-t text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
        role="grid"
        aria-label="Excluded paths"
        aria-multiselectable="true"
        aria-activedescendant={excludedList.activeId}
        tabindex="0"
        {@attach excludedList.attach}>
        {#each excluded as r, i (key(r))}
          <div
            role="row"
            {...excludedList.line(r, i)}
            class="flex items-center gap-2 list-row px-4 py-1.5">
            <XIcon class="size-4 text-danger" />
            <span role="gridcell" class="font-mono text-xs">{r.localPath}</span>
          </div>
        {:else}
          <EmptyRow text="No exclusions" />
        {/each}
      </div>
    {/snippet}
  </Card>
{/if}

<Dialog
  bind:open={() => open !== null, (o) => !o && (open = null)}
  title={open?.localPath ?? ""}
  class="w-[min(56rem,calc(100vw-2rem))]">
  {#if contentsError}
    <Notice kind="error">{contentsError}</Notice>
  {:else if contents === null}
    <Skeleton class="h-64" />
  {:else}
    <pre
      class="max-h-[60svh] overflow-auto rounded-control bg-surface-muted p-3 font-mono text-xs">{contents}</pre>
  {/if}
  {#snippet footer()}
    <Button variant="outline" disabled={!contents} onclick={() => void copyText(contents ?? "")}
      ><CopyIcon />Copy</Button>
    <Button variant="ghost" onclick={() => (open = null)}>Close</Button>
  {/snippet}
</Dialog>
