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
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
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
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Progress from "#lib/components/ui/Progress.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { valueAt } from "#lib/config/sections.js";
  import { adminView } from "#lib/config/view.svelte.js";
  import { editabilityOf, writeFromPage } from "#lib/config/write.js";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { columnWidth, formatCount, plural, WIDEST } from "#lib/format.js";
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
  import { audienceText, contentsText } from "#lib/system/shares.js";
  import {
    BUDDIES,
    USER_DEFINED,
    VISIBLE_TO,
    type VisibleTo,
    withAudience
  } from "#lib/users/groups.js";

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
      {
        id: "visible",
        label: "Visible to",
        width: "11.5rem",
        value: (r) => audienceText(r.visibleTo)
      }
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

  // who sees each share is an option, changed here when the options can be changed from a page
  const admin = $derived(session.can("Administrator"));
  const view = adminView();
  $effect(() => live.on("application.options", () => void shares.load()));
  const visibleTo = $derived((valueAt(view.data?.values, VISIBLE_TO) ?? {}) as VisibleTo);
  const settable = $derived(
    !!view.data && admin && editabilityOf(view.data, [VISIBLE_TO]).mode !== "locked"
  );
  /** the groups a share can be shown to: the buddies and the user-defined groups */
  const choices = $derived([
    BUDDIES,
    ...Object.keys(valueAt(view.data?.values, USER_DEFINED) ?? {}).toSorted()
  ]);
  let showing = $state(false);
  const showTo = async (r: Row, groups: string[] | null) => {
    const data = view.data;
    if (!data) return;
    showing = true;
    const outcome = await writeFromPage(data, {
      [VISIBLE_TO]: withAudience(visibleTo, r.alias, groups)
    });
    showing = false;
    void view.load();
    // the options are read again, so a second try is made against what changed
    if (!outcome.ok) return toast(outcome.error, "error");
    // every share on any host with the alias, as the daemon matches it
    for (const row of rows.rows)
      if (row.alias.toLowerCase() === r.alias.toLowerCase())
        rows.upsert({ ...row, visibleTo: groups });
  };
  /** `r` shown to `group` too, or no longer; with no group left no one sees it, never everyone */
  const toggle = (r: Row, group: string, on: boolean) => {
    const held = r.visibleTo ?? [];
    void showTo(r, on ? [...held, group] : held.filter((g) => g !== group));
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
            <Tooltip text={r.host} clipped>
              {#snippet child({ props })}
                <span {...props} role="gridcell" class="truncate px-2">{r.host}</span>
              {/snippet}
            </Tooltip>
            <span role="gridcell" class="min-w-0 px-2">
              <!-- shares often sit side by side in one folder, so a path is cut in the middle -->
              <Tooltip text={r.localPath} dense>
                {#snippet child({ props })}
                  <button
                    {...props}
                    type="button"
                    class="flex max-w-full items-center gap-1.5 text-left font-mono text-xs text-accent hover:underline"
                    onclick={() => (open = r)}>
                    <FolderIcon class="size-4 shrink-0" />
                    <MiddleTruncate text={r.localPath} path />
                  </button>
                {/snippet}
              </Tooltip>
              {#if r.derivedFrom}
                <span class="text-xs">
                  Shared because
                  <a href={href("/system/settings")} class="underline">Share downloads folder</a>
                  is on (<code>{r.derivedFrom}</code>)
                </span>
              {/if}
            </span>
            {#each [r.directories, r.files] as count, at (at)}
              {@const text = count == null ? "?" : formatCount(count)}
              <Tooltip {text} clipped>
                {#snippet child({ props })}
                  <span {...props} role="gridcell" class="truncate px-2 text-right tabular-nums"
                    >{text}</span>
                {/snippet}
              </Tooltip>
            {/each}
            <Tooltip text={r.alias} clipped>
              {#snippet child({ props })}
                <span {...props} role="gridcell" class="truncate px-2">{r.alias}</span>
              {/snippet}
            </Tooltip>
            <Tooltip text={r.remotePath} dense>
              {#snippet child({ props })}
                <span {...props} role="gridcell" class="flex min-w-0 px-2 font-mono text-xs">
                  <MiddleTruncate text={r.remotePath} path />
                </span>
              {/snippet}
            </Tooltip>
            <span role="gridcell" class="px-2">
              {#if settable}
                {@const held = r.visibleTo}
                <Menu.Root>
                  <Menu.Trigger disabled={showing}>
                    {#snippet child({ props })}
                      <Button
                        {...props}
                        variant="outline"
                        size="sm"
                        class="w-full justify-between"
                        aria-label={`Who sees ${r.alias}: ${audienceText(held)}`}>
                        <span class="truncate">{audienceText(held)}</span><ChevronDownIcon />
                      </Button>
                    {/snippet}
                  </Menu.Trigger>
                  <Menu.Content align="start">
                    <Menu.CheckboxItem
                      checked={held === null}
                      onCheckedChange={(on) => {
                        if (on) void showTo(r, null);
                      }}>Everyone</Menu.CheckboxItem>
                    <Menu.CheckboxItem
                      checked={held?.length === 0}
                      onCheckedChange={(on) => {
                        if (on) void showTo(r, []);
                      }}>No one</Menu.CheckboxItem>
                    <Menu.Separator />
                    {#each choices as group (group)}
                      <Menu.CheckboxItem
                        checked={!!held?.includes(group)}
                        onCheckedChange={(on) => toggle(r, group, on)}>{group}</Menu.CheckboxItem>
                    {/each}
                  </Menu.Content>
                </Menu.Root>
              {:else}
                <span class="truncate">{audienceText(r.visibleTo)}</span>
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
            <XIcon class="size-4 shrink-0 text-danger" />
            <span role="gridcell" class="min-w-0 font-mono text-xs wrap-anywhere"
              >{r.localPath}</span>
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
