<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UploadIcon from "@lucide/svelte/icons/upload";

  import { api, root, type Schemas } from "#lib/api/client.js";
  import { clock } from "#lib/clock.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { ago, dateTime, listText, plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  import { unblock, unblockMany } from "./block.svelte";
  import Username from "./Username.svelte";

  // the blocks in force: ours, and the configuration's, which only the configuration lifts
  const blocked = new Loader(() => call(() => api.GET("/api/v0/users/blocked")));
  keepCurrent(
    () => void blocked.load(),
    () => "users"
  );
  void blocked.load();
  $effect(() =>
    live.on("users.updated", (u) => {
      const was = blocked.data?.users.some((b) => b.username === u.username);
      if (was || u.block) void blocked.load();
    })
  );

  const entries = $derived(blocked.data?.users ?? []);
  const rules = $derived(blocked.data?.rules);
  const list = new ListControl<Schemas["Block"]>({
    lines: () => entries,
    key: (b) => b.username,
    // a block the configuration makes can't be lifted here, so it isn't selectable
    id: (b) => (b.source === "config" ? null : b.username)
  });
  const chosen = $derived(entries.filter((b) => list.selected.has(b.username)));
  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));

  let importing = $state(false);
  let text = $state("");
  let importError = $state("");

  const unblockChosen = async () => {
    await unblockMany(chosen.map((b) => b.username));
    list.clear();
    void blocked.load();
  };

  const runImport = async () => {
    const outcome = await call(() =>
      api.POST("/api/v0/users/blocked/import", {
        body: text,
        headers: { "Content-Type": "text/plain" },
        bodySerializer: (body) => body
      })
    );
    if (!outcome.ok) return void (importError = outcome.error);
    importing = false;
    text = "";
    toast(`${outcome.data.added} imported, ${outcome.data.skipped} skipped`);
    void blocked.load();
  };

  const COLUMNS = "2rem minmax(10rem,1fr) minmax(8rem,1fr) 14rem 8rem 8rem 3rem";
</script>

<div class="flex flex-wrap items-center gap-inline">
  <span class="text-sm text-fg-muted">{entries.length} blocked</span>
  <!-- the buttons wrap onto a second line where large text leaves no room for all three -->
  <div class="ml-auto flex flex-wrap justify-end gap-inline">
    {#if admin}
      <Button variant="outline" size="sm" onclick={() => (importing = true)}>
        <UploadIcon />Import
      </Button>
    {/if}
    <Button
      variant="outline"
      size="sm"
      href={`${root()}/api/v0/users/blocked/export?format=text`}
      download="blocked.txt">
      <DownloadIcon />Export
    </Button>
    <Button
      variant="outline"
      size="sm"
      href={`${root()}/api/v0/users/blocked/export?format=json`}
      download="blocked.json">
      <DownloadIcon />Export JSON
    </Button>
  </div>
</div>

<LoadError loader={blocked} />

{#if rules && (rules.patterns.length || rules.cidrs.length || rules.blocklistFile)}
  <Notice kind="info">
    The configuration also blocks
    {listText(
      [
        rules.patterns.length && plural(rules.patterns.length, "name pattern", "name patterns"),
        rules.cidrs.length && plural(rules.cidrs.length, "address range", "address ranges"),
        rules.blocklistFile &&
          `${plural(rules.blocklistFile.entries, "entry", "entries")} from ${rules.blocklistFile.path}`
      ].filter((p): p is string => !!p)
    )}. You can change these in Settings, under Groups.
  </Notice>
{/if}

{#if blocked.loading}
  <Skeleton class="h-40" />
{:else if !entries.length}
  <EmptyState icon={ShieldCheckIcon} title="No blocked users" />
{:else}
  <div
    class="overflow-x-auto rounded-card border bg-surface text-sm shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Blocked users"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}>
    <div
      role="row"
      class="grid h-9 min-w-max items-center gap-2 border-b bg-surface-muted px-2 text-xs font-medium text-fg-muted"
      style:grid-template-columns={COLUMNS}>
      <span role="columnheader"></span>
      <span role="columnheader">User</span>
      <span role="columnheader">Reason</span>
      <span role="columnheader">Stops</span>
      <span role="columnheader">Ends</span>
      <span role="columnheader">Since</span>
      <span role="columnheader" class="sr-only">Actions</span>
    </div>
    <div role="rowgroup" {@attach freezesWhileUsed}>
      {#each entries as b, i (b.username)}
        <div
          role="row"
          {...list.line(b, i)}
          class={[
            "grid min-w-max items-center gap-2 list-row px-2 py-1.5 hover:bg-hover",
            list.selected.has(b.username) && "bg-accent-soft/60"
          ]}
          style:grid-template-columns={COLUMNS}>
          <span role="gridcell">
            {#if b.source !== "config"}
              <Checkbox
                aria-label={`Select ${b.username}`}
                checked={list.selected.has(b.username)}
                onclick={(e) => {
                  e.preventDefault();
                  list.tick(b.username, e.shiftKey);
                }}
                onCheckedChange={(on) =>
                  on !== list.selected.has(b.username) && list.toggle(b.username)} />
            {/if}
          </span>
          <span role="gridcell" class="flex min-w-0 items-center gap-1.5">
            <Username name={b.username} />
            {#if b.source === "config"}<Badge tone="outline">configuration</Badge>{/if}
          </span>
          <span role="gridcell" class="truncate" title={b.reason}>{b.reason}</span>
          <span role="gridcell" class="truncate text-xs text-fg-muted">{b.scopes.join(", ")}</span>
          <span
            role="gridcell"
            class="text-xs"
            title={b.expiresAt ? dateTime(b.expiresAt) : undefined}>
            {b.expiresAt ? ago(b.expiresAt, clock.now) : "never"}
          </span>
          <span
            role="gridcell"
            class="text-xs text-fg-muted"
            title={b.createdAt ? dateTime(b.createdAt) : undefined}>
            {b.createdAt ? ago(b.createdAt, clock.now) : ""}
          </span>
          <span role="gridcell" class="flex justify-end">
            {#if write && b.source === "api"}
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Unblock ${b.username}`}
                onclick={() => void unblock(b.username)}>
                <ShieldCheckIcon />
              </Button>
            {/if}
          </span>
        </div>
      {/each}
    </div>
  </div>
{/if}

{#if list.selected.size && write}
  <div class="selection-bar" role="toolbar" aria-label="Selected blocks">
    <span class="text-sm">{chosen.length} selected</span>
    <Button size="sm" variant="outline" onclick={() => void unblockChosen()}>Unblock</Button>
    <Button size="sm" variant="ghost" onclick={() => list.clear()}>Clear</Button>
  </div>
{/if}

<Dialog
  bind:open={importing}
  title="Import blocks"
  description="One username per line. Lines starting with # are ignored. You can also paste a JSON export.">
  <Textarea
    class="min-h-40 font-mono text-xs"
    aria-label="Blocks to import"
    spellcheck="false"
    bind:value={text} />
  {#if importError}<Notice kind="error">{importError}</Notice>{/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (importing = false)}>Cancel</Button>
    <Button disabled={!text.trim()} onclick={() => void runImport()}>Import</Button>
  {/snippet}
</Dialog>
