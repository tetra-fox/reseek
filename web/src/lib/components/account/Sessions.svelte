<script lang="ts" module>
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the sessions' sort and column widths, kept in this browser; the daemon's order until a sort */
  const view = gridView("reseek.account.sessions", ["who", "role", "created", "seen"], null);
</script>

<script lang="ts">
  import LogOutIcon from "@lucide/svelte/icons/log-out";
  import { onMount } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { ago, dateTime } from "#lib/format.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { Grid, keptIn } from "#lib/lists/table.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  type Session = Schemas["Session"];
  const rows = new Keyed<Session>((s) => s.id);
  const loader = new Loader(async () => {
    const outcome = await call(() => api.GET("/api/v0/sessions"));
    if (outcome.ok) rows.reconcile(outcome.data);
    return outcome;
  });
  onMount(() => void loader.load());

  const busy = new Busy();
  let everywhere = $state(false);
  const admin = $derived(session.can("Administrator"));

  const revoke = (id: string) =>
    busy.run(async () => {
      const outcome = await call(() =>
        api.DELETE("/api/v0/sessions/{id}", { params: { path: { id } } })
      );
      if (outcome.ok) await loader.load();
      return outcome;
    }, "Session logged out");

  const revokeOthers = () =>
    busy.run(async () => {
      const outcome = await call(() => api.DELETE("/api/v0/sessions"));
      if (outcome.ok) await loader.load();
      return outcome;
    }, "Other sessions logged out");

  // a phone keeps the name and the log-out button, with the role and last use under the name
  const grid = new Grid<Session>({
    columns: () => [
      { id: "who", label: "Who", width: "10rem", grow: true, value: (s) => s.name.toLowerCase() },
      { id: "role", label: "Role", width: "7.5rem", value: (s) => s.role },
      {
        id: "created",
        label: "Logged in",
        width: "10.5rem",
        descFirst: true,
        value: (s) => Date.parse(s.createdAt)
      },
      {
        id: "seen",
        label: "Last used",
        width: "8.5rem",
        descFirst: true,
        value: (s) => Date.parse(s.lastSeenAt)
      },
      { id: "from", label: "From", width: "10rem" },
      { id: "actions", label: "Actions", width: "3rem", quiet: true, fixed: true }
    ],
    data: () => rows.rows,
    ...keptIn(view),
    shown: () =>
      viewport.phone ? ["who", "actions"] : ["who", "role", "created", "seen", "from", "actions"],
    clearable: true
  });
  const shown = $derived(grid.rows);
  const list = new ListControl<Session>({ lines: () => shown, key: (s) => s.id, id: (s) => s.id });

  /** where a session was last used from, with the browser it was used in */
  const fromText = (s: Session) =>
    [s.ip ?? "Address unknown", s.userAgent].filter(Boolean).join("\n");
</script>

<Card title="Sessions">
  {#snippet action()}
    {#if admin}
      <Button
        variant="outline"
        size="sm"
        disabled={busy.running}
        onclick={() => (everywhere = true)}>
        Log out other sessions
      </Button>
    {/if}
  {/snippet}
  <LoadError {loader} />
  {#snippet flush()}
    <div
      class="overflow-x-auto border-t text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
      role="grid"
      aria-label="Sessions"
      aria-multiselectable="true"
      aria-activedescendant={list.activeId}
      tabindex="0"
      {@attach list.attach}>
      <GridHeader {grid} class="h-8 border-b bg-surface-muted text-fg-muted" />
      {#each shown as s, i (s.id)}
        <div
          role="row"
          {...list.line(s, i)}
          class="grid items-center list-row py-1.5"
          style={grid.row}>
          <span role="gridcell" class="flex min-w-0 flex-col px-2">
            <span class="flex min-w-0 items-center gap-1.5">
              <Tooltip text={s.name} clipped>
                {#snippet child({ props })}
                  <span {...props} class="truncate" dir="auto">{s.name}</span>
                {/snippet}
              </Tooltip>
              {#if s.current}<Badge tone="accent">this browser</Badge>{/if}
            </span>
            {#if viewport.phone}
              <span class="truncate text-xs text-fg-muted"
                >{s.role}, last used {ago(s.lastSeenAt, clock.now)}</span>
            {/if}
          </span>
          {#if !viewport.phone}
            <span role="gridcell" class="truncate px-2">{s.role}</span>
            <span role="gridcell" class="truncate px-2">{dateTime(s.createdAt)}</span>
            <Tooltip text={dateTime(s.lastSeenAt)} dense>
              {#snippet child({ props })}
                <span {...props} role="gridcell" class="truncate px-2"
                  >{ago(s.lastSeenAt, clock.now)}</span>
              {/snippet}
            </Tooltip>
            <Tooltip text={fromText(s)} dense>
              {#snippet child({ props })}
                <span {...props} role="gridcell" class="flex min-w-0 px-2">
                  <MiddleTruncate text={s.ip ?? "?"} />
                </span>
              {/snippet}
            </Tooltip>
          {/if}
          <span role="gridcell" class="flex min-h-control-sm justify-end px-1">
            {#if !s.current}
              <Tooltip text={`Log out ${s.name}'s session`}>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Log out ${s.name}'s session`}
                    disabled={busy.running}
                    onclick={() => revoke(s.id)}>
                    <LogOutIcon />
                  </Button>
                {/snippet}
              </Tooltip>
            {/if}
          </span>
        </div>
      {:else}
        {#if loader.data}<EmptyRow text="No sessions" />{/if}
      {/each}
    </div>
  {/snippet}
</Card>

<ConfirmDialog
  bind:open={everywhere}
  title="Log out other sessions?"
  body="Other browsers will have to log in again"
  confirm="Log out other sessions"
  danger
  onconfirm={revokeOthers} />
