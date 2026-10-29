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
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { contentsText } from "#lib/system/shares.js";

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

  const shared = $derived(rows.rows.filter((r) => !r.isExcluded));
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

  const COLUMNS = "6rem minmax(12rem,2fr) 6rem 6rem minmax(6rem,1fr) minmax(6rem,1fr)";
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
    class="flex flex-col gap-2 rounded-card border bg-surface p-4 shadow-card"
    role="status"
    aria-live="polite">
    <div class="flex items-baseline gap-3">
      <span class="text-sm font-medium">Scanning shares</span>
      <span class="ml-auto text-sm font-semibold tabular-nums">{percent}%</span>
    </div>
    <div
      class="h-1.5 overflow-hidden rounded-full bg-surface-muted"
      role="progressbar"
      aria-label="Scan progress"
      aria-valuenow={percent}
      aria-valuemin={0}
      aria-valuemax={100}>
      <div
        class="h-full rounded-full bg-accent transition-[width] duration-(--motion-slow)"
        style:width={`${percent}%`}>
      </div>
    </div>
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
        <div
          role="row"
          class="grid min-w-max gap-2 border-b bg-surface-muted px-4 py-1.5 text-xs font-medium text-fg-muted"
          style:grid-template-columns={COLUMNS}>
          {#each ["Host", "Local path", "Folders", "Files", "Alias", "Remote path"] as h (h)}
            <span role="columnheader">{h}</span>
          {/each}
        </div>
        {#each shared as r, i (key(r))}
          <div
            role="row"
            {...sharedList.line(r, i)}
            class={[
              "grid min-w-max items-center gap-2 list-row px-4 py-1.5 hover:bg-hover",
              r.derivedFrom && "text-fg-muted",
              sharedList.selected.has(key(r)) && "bg-accent-soft/60"
            ]}
            style:grid-template-columns={COLUMNS}>
            <span role="gridcell">{r.host}</span>
            <span role="gridcell" class="min-w-0">
              <button
                type="button"
                class="flex items-center gap-1.5 text-left text-accent hover:underline"
                onclick={() => (open = r)}>
                <FolderIcon class="size-4 shrink-0" /><span class="truncate font-mono text-xs"
                  >{r.localPath}</span>
              </button>
              {#if r.derivedFrom}
                <span class="text-xs">
                  Shared because
                  <a href={href("/system/settings")} class="underline"
                    >Share the downloads folder</a>
                  is enabled (<code>{r.derivedFrom}</code>)
                </span>
              {/if}
            </span>
            <span role="gridcell" class="tabular-nums"
              >{r.directories?.toLocaleString() ?? "?"}</span>
            <span role="gridcell" class="tabular-nums">{r.files?.toLocaleString() ?? "?"}</span>
            <span role="gridcell" class="truncate">{r.alias}</span>
            <span role="gridcell" class="truncate font-mono text-xs">{r.remotePath}</span>
          </div>
        {:else}
          <p class="px-4 py-6 text-center text-sm text-fg-subtle">No shares configured</p>
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
            class={[
              "flex items-center gap-2 list-row px-4 py-1.5 hover:bg-hover",
              excludedList.selected.has(key(r)) && "bg-accent-soft/60"
            ]}>
            <XIcon class="size-4 text-danger" />
            <span role="gridcell" class="font-mono text-xs">{r.localPath}</span>
          </div>
        {:else}
          <p class="px-4 py-6 text-center text-sm text-fg-subtle">No exclusions configured</p>
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
