<script lang="ts" module>
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the blocked list's sort and column widths, kept in this browser */
  const view = gridView("reseek.users.blocked", ["user", "reason", "ends", "since"], null);
</script>

<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UploadIcon from "@lucide/svelte/icons/upload";

  import { api, root, type Schemas } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Section from "#lib/components/settings/Section.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { BLOCKING } from "#lib/config/sections.js";
  import { adminView } from "#lib/config/view.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, dateTime, formatCount, listText, plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { Grid, keptIn } from "#lib/lists/table.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { footBars } from "#lib/ui/footbars.svelte.js";
  import { configBlocked } from "#lib/users/actions.js";
  import { blockEnded, unblock, unblockMany } from "#lib/users/block.svelte.js";

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

  // the configuration's blocks come and go with the options
  $effect(() => live.on("application.options", () => void blocked.load()));

  const time = (iso: string | null | undefined, none: number) => (iso ? Date.parse(iso) : none);
  const grid = new Grid<Schemas["Block"]>({
    columns: () => [
      { id: "select", label: "Select", width: "2.5rem", quiet: true, fixed: true },
      {
        id: "user",
        label: "User",
        width: "10rem",
        grow: true,
        value: (b) => b.username.toLowerCase()
      },
      {
        id: "reason",
        label: "Reason",
        width: "8rem",
        grow: true,
        value: (b) => b.reason.toLowerCase()
      },
      { id: "stops", label: "Stops", width: "14rem" },
      {
        id: "ends",
        label: "Ends",
        width: "8rem",
        value: (b) => time(b.expiresAt, Number.MAX_SAFE_INTEGER)
      },
      {
        id: "since",
        label: "Since",
        width: "8rem",
        descFirst: true,
        value: (b) => time(b.createdAt, 0)
      },
      { id: "actions", label: "Actions", width: "3rem", quiet: true, fixed: true }
    ],
    data: () => blocked.data?.users ?? [],
    ...keptIn(view),
    clearable: true,
    ties: "user"
  });
  const entries = $derived(grid.rows);
  const rules = $derived(blocked.data?.rules);
  const list = new ListControl<Schemas["Block"]>({
    lines: () => entries,
    key: (b) => b.username,
    // a block the configuration makes can't be lifted here, so it isn't selectable
    id: (b) => (configBlocked(b) ? null : b.username)
  });
  const chosen = $derived(entries.filter((b) => list.selected.has(b.username)));
  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));
  const options = adminView();

  let importing = $state(false);
  let text = $state("");
  let importError = $state("");
  let sending = $state(false);

  // each opening of the import starts without the last one's error
  $effect(() => {
    if (importing) importError = "";
  });

  const unblockChosen = async () => {
    await unblockMany(chosen.map((b) => b.username));
    list.clear();
    void blocked.load();
  };

  const runImport = async () => {
    if (sending) return;
    sending = true;
    const outcome = await call(() =>
      api.POST("/api/v0/users/blocked/import", {
        body: text,
        headers: { "Content-Type": "text/plain" },
        bodySerializer: (body) => body
      })
    );
    sending = false;
    if (!outcome.ok) return void (importError = outcome.error);
    importing = false;
    text = "";
    toast(
      `${formatCount(outcome.data.added)} imported, ${formatCount(outcome.data.skipped)} skipped`
    );
    void blocked.load();
  };
</script>

<div class="flex flex-wrap items-center gap-inline">
  <span class="text-sm text-fg-muted">{formatCount(entries.length)} blocked</span>
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
    )}. These rules are set in Settings, under Groups.
  </Notice>
{/if}

{#if blocked.loading}
  <Skeleton class="h-40" />
{:else if !entries.length}
  <EmptyState icon={ShieldCheckIcon} title="No blocked users" />
{:else}
  <div
    class="overflow-x-auto rounded-card border bg-surface text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Blocked users"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}>
    <GridHeader {grid} all={list.allTick} class="h-9 border-b bg-surface-muted text-fg-muted" />
    <div role="rowgroup" {@attach freezesWhileUsed}>
      {#each entries as b, i (b.username)}
        <div
          role="row"
          {...list.line(b, i)}
          class="grid items-center list-row py-1.5"
          style={grid.row}>
          <span role="gridcell" class="flex px-2">
            {#if !configBlocked(b)}
              <Checkbox
                aria-label={`Select ${b.username}`}
                checked={list.selected.has(b.username)}
                ontick={(range) => list.tick(b.username, range)} />
            {/if}
          </span>
          <span role="gridcell" class="flex min-w-0 items-center gap-1.5 px-2">
            <Username name={b.username} />
            {#if b.source === "config"}<Badge tone="outline">configuration</Badge>{/if}
            {#if b.addresses.length}
              <!-- the addresses take what the name leaves -->
              <Tooltip text={`Also blocked from ${b.addresses.join(", ")}`} dense>
                {#snippet child({ props })}
                  <span {...props} class="grow basis-0 truncate font-mono text-xs text-fg-muted"
                    >{b.addresses.join(", ")}</span>
                {/snippet}
              </Tooltip>
            {/if}
          </span>
          <Tooltip text={b.reason} clipped>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate px-2">{b.reason}</span>
            {/snippet}
          </Tooltip>
          <Tooltip text={b.scopes.join(", ")} clipped>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate px-2 text-xs text-fg-muted"
                >{b.scopes.join(", ")}</span>
            {/snippet}
          </Tooltip>
          <Tooltip text={b.expiresAt ? dateTime(b.expiresAt) : undefined} dense>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate px-2 text-xs">
                {!b.expiresAt
                  ? "never"
                  : blockEnded(b, clock.now)
                    ? "expired"
                    : ago(b.expiresAt, clock.now)}
              </span>
            {/snippet}
          </Tooltip>
          <Tooltip text={b.createdAt ? dateTime(b.createdAt) : undefined} dense>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate px-2 text-xs text-fg-muted">
                {b.createdAt ? ago(b.createdAt, clock.now) : ""}
              </span>
            {/snippet}
          </Tooltip>
          <span role="gridcell" class="flex justify-end px-1">
            {#if write && !configBlocked(b)}
              <Tooltip text="Unblock">
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Unblock ${b.username}`}
                    onclick={() => void unblock(b.username)}>
                    <ShieldCheckIcon />
                  </Button>
                {/snippet}
              </Tooltip>
            {/if}
          </span>
        </div>
      {/each}
    </div>
  </div>
{/if}

{#if options.data}
  <!-- the blocking options, which the list above shows the effect of -->
  <Section section={BLOCKING} view={options.data} onchanged={options.load} />
{/if}

{#if list.selected.size && write}
  <div class="selection-bar" role="toolbar" aria-label="Selected blocks" {@attach footBars.attach}>
    <span class="text-sm">{formatCount(chosen.length)} selected</span>
    <Button size="sm" variant="outline" onclick={() => void unblockChosen()}>Unblock</Button>
    <Button size="sm" variant="ghost" onclick={() => list.clear()}>Clear</Button>
  </div>
{/if}

<Dialog
  bind:open={importing}
  busy={sending}
  title="Import blocks"
  description="One username per line, or a JSON export. Lines starting with # are ignored.">
  <Textarea
    class="min-h-40 font-mono text-xs"
    aria-label="Blocks to import"
    spellcheck="false"
    disabled={sending}
    bind:value={text} />
  {#if importError}<Notice kind="error">{importError}</Notice>{/if}
  {#snippet footer()}
    <Button variant="ghost" disabled={sending} onclick={() => (importing = false)}>Cancel</Button>
    <Button disabled={!text.trim() || sending} onclick={() => void runImport()}>Import</Button>
  {/snippet}
</Dialog>
