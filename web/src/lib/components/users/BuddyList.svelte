<script lang="ts">
  import { goto } from "$app/navigation";
  import UserMinusIcon from "@lucide/svelte/icons/user-minus";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";
  import UsersIcon from "@lucide/svelte/icons/users";

  import { api } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Flag from "#lib/components/ui/Flag.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { chatTime, dateTime, formatSpeed, plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { befriend, reach, setBuddy } from "#lib/users/actions.js";
  import { pictureOf } from "#lib/users/profile.svelte.js";
  import type { UserSummary } from "#lib/users/users.svelte.js";
  import { BUDDY } from "#lib/wording.js";

  import Avatar from "./Avatar.svelte";
  import Presence from "./Presence.svelte";
  import Username from "./Username.svelte";

  // the buddy list: every buddy by name with their presence as the server tells it, kept current
  // by the users topic, so a buddy added or changed anywhere shows here at once

  const rows = new Keyed<UserSummary>((u) => u.username);
  let loaded = $state(false);
  let error = $state("");

  const load = async () => {
    const turn = rows.reading();
    const found: UserSummary[] = [];
    for (let offset = 0; ;) {
      const outcome = await call(() =>
        api.GET("/api/v0/users", {
          params: { query: { buddy: true, sort: "username", order: "asc", offset, limit: 500 } }
        })
      );
      if (!outcome.ok) {
        if (!rows.abandon(turn)) return;
        error = outcome.error;
        loaded = true;
        return;
      }
      if (!rows.latest(turn)) return;
      found.push(...outcome.data.users);
      offset += outcome.data.users.length;
      if (!outcome.data.users.length || offset >= outcome.data.total) break;
    }
    rows.reconcile(found, turn);
    error = "";
    loaded = true;
  };

  keepCurrent(
    () => void load(),
    () => "users"
  );
  void load();
  $effect(() =>
    live.on("users.updated", (summary) => {
      if (summary.buddy) rows.upsert(summary);
      else if (rows.get(summary.username)) rows.remove(summary.username);
    })
  );
  $effect(() => live.on("users.removed", ({ username }) => rows.remove(username)));

  const shown = $derived(rows.rows.toSorted((a, b) => a.username.localeCompare(b.username)));
  const online = $derived(shown.filter((u) => u.presence && u.presence !== "Offline").length);
  const write = $derived(session.can("ReadWrite"));

  const list = new ListControl<UserSummary>({
    lines: () => shown,
    key: (u) => u.username,
    id: (u) => u.username,
    onopen: (u) => void goto(userHref(u.username))
  });

  let adding = $state("");
  let busy = $state(false);
  const add = async () => {
    const name = adding.trim();
    if (!name || busy) return;
    busy = true;
    const outcome = await setBuddy(name, true);
    busy = false;
    if (!outcome.ok) return toast(outcome.error, "error");
    adding = "";
  };
  const change = async (run: () => ReturnType<typeof befriend>) => {
    const outcome = await run();
    if (!outcome.ok) toast(outcome.error, "error");
  };

  /** the presence and since when it holds: the time of day for today, with the date before */
  const since = (u: UserSummary) =>
    u.presence && u.presenceAt
      ? `${u.presence} since ${chatTime(u.presenceAt, clock.now)}`
      : "Unknown";

  const COLUMNS = "minmax(11rem,1fr) 14rem 6rem 6rem minmax(8rem,1fr) 5rem 5rem 6rem";
</script>

<div class="flex flex-wrap items-center gap-inline">
  {#if write}
    <form
      class="flex items-center gap-inline"
      onsubmit={(e) => {
        e.preventDefault();
        void add();
      }}>
      <Input
        class="h-control-sm w-48"
        aria-label="Username to add as a buddy"
        placeholder="Username"
        autocomplete="off"
        spellcheck="false"
        bind:value={adding} />
      <Button type="submit" size="sm" variant="outline" disabled={!adding.trim() || busy}
        ><UserPlusIcon />{BUDDY.add}</Button>
    </form>
  {/if}
  {#if shown.length}
    <span class="ml-auto text-sm text-fg-muted"
      >{online} of {plural(shown.length, "buddy", "buddies")} online</span>
  {/if}
</div>

<LoadError loader={{ error, load }} />

{#if !loaded}
  <Skeleton class="h-40" />
{:else if !shown.length}
  <EmptyState
    icon={UsersIcon}
    title="No buddies yet"
    description="Add a buddy to see when they are online and to share folders with buddies only" />
{:else}
  <div
    class="overflow-x-auto rounded-card border bg-surface text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label={BUDDY.plural}
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}>
    <div
      role="row"
      class="grid h-9 min-w-max items-center gap-2 border-b bg-surface-muted px-2 text-xs font-medium text-fg-muted"
      style:grid-template-columns={COLUMNS}>
      <span role="columnheader">User</span>
      <span role="columnheader">Presence</span>
      <span role="columnheader">Speed</span>
      <span role="columnheader">Files</span>
      <span role="columnheader">Note</span>
      <Tooltip text={BUDDY.trustedHint}>
        {#snippet child({ props })}<span {...props} role="columnheader">{BUDDY.trusted}</span
          >{/snippet}
      </Tooltip>
      <Tooltip text={BUDDY.notifyHint}>
        {#snippet child({ props })}<span {...props} role="columnheader">Notify</span>{/snippet}
      </Tooltip>
      <span role="columnheader" class="sr-only">Actions</span>
    </div>
    <div role="rowgroup" {@attach freezesWhileUsed}>
      {#each shown as u, i (u.username)}
        {@const buddy = u.buddy ?? { trusted: false, notifyPresence: false }}
        <div
          role="row"
          {...list.line(u, i)}
          class="grid min-w-max items-center gap-2 list-row px-2 py-1.5"
          style:grid-template-columns={COLUMNS}>
          <span role="gridcell" class="flex w-0 min-w-full items-center gap-2">
            <Avatar
              name={u.username}
              pictureUrl={pictureOf(u.username, u)}
              class="size-7 text-xs" />
            <Username name={u.username} />
            {#if u.block}<Badge tone="danger">blocked</Badge>{/if}
          </span>
          <Tooltip text={u.presenceAt ? dateTime(u.presenceAt) : undefined} dense>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="flex items-center gap-1.5 text-xs">
                <Presence presence={u.presence} worded />{since(u)}
                <Flag code={u.countryCode} />
              </span>
            {/snippet}
          </Tooltip>
          <span role="gridcell" class="text-xs tabular-nums"
            >{u.statistics ? formatSpeed(u.statistics.averageSpeed) : ""}</span>
          <span role="gridcell" class="text-xs tabular-nums"
            >{u.statistics?.fileCount.toLocaleString() ?? ""}</span>
          <Tooltip text={u.notes ?? undefined} clipped>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate text-xs text-fg-muted"
                >{u.notes ?? ""}</span>
            {/snippet}
          </Tooltip>
          <span role="gridcell">
            <Switch
              aria-label={`${BUDDY.trusted}: ${u.username}`}
              checked={buddy.trusted}
              disabled={!write}
              onCheckedChange={(on) => void change(() => befriend(u.username, { trusted: on }))} />
          </span>
          <span role="gridcell">
            <Switch
              aria-label={`${BUDDY.notify}: ${u.username}`}
              checked={buddy.notifyPresence}
              disabled={!write}
              onCheckedChange={(on) =>
                void change(() => befriend(u.username, { notifyPresence: on }))} />
          </span>
          <span role="gridcell" class="flex justify-end">
            {#each reach(u.username, { only: ["message", "shares"] }) as action (action.id)}
              <Tooltip text={action.label}>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    href={action.href}
                    aria-label={action.of(u.username)}><action.icon /></Button>
                {/snippet}
              </Tooltip>
            {/each}
            {#if write}
              <Tooltip text={BUDDY.remove}>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`${BUDDY.remove}: ${u.username}`}
                    onclick={() => void change(() => setBuddy(u.username, false))}
                    ><UserMinusIcon /></Button>
                {/snippet}
              </Tooltip>
            {/if}
          </span>
        </div>
      {/each}
    </div>
  </div>
{/if}
