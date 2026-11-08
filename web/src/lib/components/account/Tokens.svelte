<script lang="ts" module>
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the tokens' sort and column widths, kept in this browser; the daemon's order until a sort */
  const view = gridView(
    "reseek.account.tokens",
    ["name", "role", "created", "expires", "used"],
    null
  );
</script>

<script lang="ts">
  import PlusIcon from "@lucide/svelte/icons/plus";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { onMount } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { ago, dateTime } from "#lib/format.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { Grid, keptIn } from "#lib/lists/table.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  import NewToken from "./NewToken.svelte";

  type Listed = Schemas["ListedToken"];
  type ApiToken = Extract<Listed, { source: "api" }>;
  const keyOf = (t: Listed) => (t.source === "api" ? t.id : `config:${t.name}`);

  const rows = new Keyed<Listed>(keyOf);
  const loader = new Loader(async () => {
    const outcome = await call(() => api.GET("/api/v0/tokens"));
    if (outcome.ok) rows.reconcile(outcome.data);
    return outcome;
  });
  onMount(() => void loader.load());

  const busy = new Busy();
  let creating = $state(false);

  const revoke = (id: string, name: string) =>
    busy.run(async () => {
      const outcome = await call(() =>
        api.DELETE("/api/v0/tokens/{id}", { params: { path: { id } } })
      );
      if (outcome.ok) await loader.load();
      return outcome;
    }, `${name} revoked`);

  /** an instant of an API token for sorting; a key from the configuration has none */
  const time = (t: Listed, at: (t: ApiToken) => string | null | undefined) => {
    const iso = t.source === "api" ? at(t) : null;
    return iso ? Date.parse(iso) : 0;
  };
  // a phone keeps the name and the revoke button, with the role and expiry under the name
  const grid = new Grid<Listed>({
    columns: () => [
      { id: "name", label: "Name", width: "10rem", grow: true, value: (t) => t.name.toLowerCase() },
      { id: "prefix", label: "Prefix", width: "8.5rem" },
      { id: "role", label: "Role", width: "7.5rem", value: (t) => t.role },
      {
        id: "created",
        label: "Created",
        width: "10.5rem",
        descFirst: true,
        value: (t) => time(t, (a) => a.createdAt)
      },
      {
        id: "expires",
        label: "Expires",
        width: "10.5rem",
        // a token that never expires sorts last
        value: (t) => time(t, (a) => a.expiresAt) || Number.MAX_SAFE_INTEGER
      },
      {
        id: "used",
        label: "Last used",
        width: "8.5rem",
        descFirst: true,
        value: (t) => time(t, (a) => a.lastUsedAt)
      },
      { id: "actions", label: "Actions", width: "3rem", quiet: true, fixed: true }
    ],
    data: () => rows.rows,
    ...keptIn(view),
    shown: () =>
      viewport.phone
        ? ["name", "actions"]
        : ["name", "prefix", "role", "created", "expires", "used", "actions"],
    clearable: true
  });
  const shown = $derived(grid.rows);
  const list = new ListControl<Listed>({ lines: () => shown, key: keyOf, id: keyOf });

  const expired = (t: ApiToken) => !!t.expiresAt && Date.parse(t.expiresAt) <= clock.now;
  /** a token's expiry in a few words, for the line under its name */
  const expiryText = (t: ApiToken) =>
    !t.expiresAt ? "never expires" : expired(t) ? "expired" : `expires ${dateTime(t.expiresAt)}`;
</script>

{#snippet expiry(t: ApiToken)}
  {#if !t.expiresAt}
    Never
  {:else if expired(t)}
    <Tooltip text={dateTime(t.expiresAt)} dense>
      {#snippet child({ props })}<span {...props} class="text-danger">Expired</span>{/snippet}
    </Tooltip>
  {:else}
    {dateTime(t.expiresAt)}
  {/if}
{/snippet}

<Card
  title="API tokens"
  description="For scripts and other tools, with no more access than the account that created them">
  {#snippet action()}
    <Button variant="outline" size="sm" onclick={() => (creating = true)}>
      <PlusIcon />New token
    </Button>
  {/snippet}
  <LoadError {loader} />
  {#snippet flush()}
    <div
      class="overflow-x-auto border-t text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
      role="grid"
      aria-label="API tokens"
      aria-multiselectable="true"
      aria-activedescendant={list.activeId}
      tabindex="0"
      {@attach list.attach}>
      <GridHeader {grid} class="h-8 border-b bg-surface-muted text-fg-muted" />
      {#each shown as t, i (keyOf(t))}
        <div
          role="row"
          {...list.line(t, i)}
          class={["grid items-center list-row py-1.5", t.source === "config" && "text-fg-muted"]}
          style={grid.row}>
          <span role="gridcell" class="flex min-w-0 flex-col px-2">
            <span class="flex min-w-0 items-center gap-1.5">
              <Tooltip text={t.name} clipped>
                {#snippet child({ props })}
                  <span {...props} class="truncate" dir="auto">{t.name}</span>
                {/snippet}
              </Tooltip>
              <!-- a key from the configuration is listed for reference; it is changed there -->
              {#if t.source === "config"}<Badge tone="outline">config</Badge>{/if}
            </span>
            {#if viewport.phone}
              <span class="truncate text-xs text-fg-muted"
                >{t.role}, {t.source === "api" ? expiryText(t) : `from ${t.cidr}`}</span>
            {/if}
          </span>
          {#if !viewport.phone}
            <span role="gridcell" class="truncate px-2">
              {#if t.source === "api"}<code class="text-xs">slsk_{t.prefix}...</code>{/if}
            </span>
            <span role="gridcell" class="truncate px-2">{t.role}</span>
            {#if t.source === "api"}
              <span role="gridcell" class="truncate px-2">{dateTime(t.createdAt)}</span>
              <span role="gridcell" class="truncate px-2">{@render expiry(t)}</span>
              {#if t.lastUsedAt}
                {@const used = t.lastUsedAt}
                <Tooltip
                  text={`${dateTime(used)} from ${t.lastUsedIp ?? "an unknown address"}`}
                  dense>
                  {#snippet child({ props })}
                    <span {...props} role="gridcell" class="truncate px-2"
                      >{ago(used, clock.now)}</span>
                  {/snippet}
                </Tooltip>
              {:else}
                <span role="gridcell" class="truncate px-2">Never</span>
              {/if}
            {:else}
              <Tooltip text={`From ${t.cidr}`} clipped>
                {#snippet child({ props })}
                  <span {...props} role="gridcell" class="col-span-3 truncate px-2"
                    >From {t.cidr}</span>
                {/snippet}
              </Tooltip>
            {/if}
          {/if}
          <span role="gridcell" class="flex min-h-control-sm justify-end px-1">
            {#if t.source === "api"}
              <Tooltip text={`Revoke ${t.name}`}>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Revoke ${t.name}`}
                    disabled={busy.running}
                    onclick={() => revoke(t.id, t.name)}>
                    <Trash2Icon />
                  </Button>
                {/snippet}
              </Tooltip>
            {/if}
          </span>
        </div>
      {:else}
        {#if loader.data}<EmptyRow text="No API tokens" />{/if}
      {/each}
    </div>
  {/snippet}
</Card>

<NewToken bind:open={creating} oncreated={() => void loader.load()} />
