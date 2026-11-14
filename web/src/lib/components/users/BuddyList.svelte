<script lang="ts" module>
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the buddy list's sort and column widths, kept in this browser */
  const view = gridView("reseek.users.buddies", ["user", "presence", "speed", "files"], {
    id: "user",
    dir: "asc"
  });
</script>

<script lang="ts">
  import { goto } from "$app/navigation";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import UserMinusIcon from "@lucide/svelte/icons/user-minus";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";
  import UsersIcon from "@lucide/svelte/icons/users";

  import MessageAll from "#lib/components/chat/MessageAll.svelte";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Flag from "#lib/components/ui/Flag.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import {
    chatTime,
    columnWidth,
    dateTime,
    formatCount,
    formatSpeed,
    plural,
    WIDEST
  } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { Grid, keptIn } from "#lib/lists/table.svelte.js";
  import { ListWindow } from "#lib/lists/virtual.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { rem } from "#lib/ui/viewport.svelte.js";
  import { befriend, reach, setBuddy } from "#lib/users/actions.js";
  import { pictureOf } from "#lib/users/profile.svelte.js";
  import { users, type UserSummary } from "#lib/users/users.svelte.js";
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
    const outcome = await users.every({ buddy: true, sort: "username", order: "asc" });
    if (!outcome.ok) {
      if (!rows.abandon(turn)) return;
      error = outcome.error;
      loaded = true;
      return;
    }
    if (!rows.latest(turn)) return;
    rows.reconcile(outcome.data, turn);
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

  /** online first, then away, then offline or not known */
  const PRESENCE_ORDER: Record<string, number> = { Online: 0, Away: 1 };
  const grid = new Grid<UserSummary>({
    columns: () => [
      {
        id: "user",
        label: "User",
        width: "11rem",
        grow: true,
        value: (u) => u.username.toLowerCase()
      },
      {
        id: "presence",
        label: "Presence",
        width: "14rem",
        value: (u) => PRESENCE_ORDER[u.presence ?? ""] ?? 2
      },
      {
        id: "speed",
        label: "Speed",
        width: columnWidth(WIDEST.speed),
        end: true,
        descFirst: true,
        value: (u) => u.statistics?.averageSpeed ?? -1
      },
      {
        id: "files",
        label: "Files",
        width: columnWidth(WIDEST.count),
        end: true,
        descFirst: true,
        value: (u) => u.statistics?.fileCount ?? -1
      },
      // narrow until there is room, so a row fits beside the sidebar on a 1280px screen
      { id: "note", label: "Note", width: "5rem", grow: true },
      { id: "notify", label: "Notify", width: "5rem", hint: BUDDY.notifyHint },
      { id: "actions", label: "Actions", width: "6rem", quiet: true, fixed: true }
    ],
    data: () => rows.rows,
    ...keptIn(view),
    ties: "user"
  });
  const shown = $derived(grid.rows);
  const online = $derived(
    shown.filter((u) => u.presence && u.presence !== "Offline").map((u) => u.username)
  );
  const write = $derived(session.can("ReadWrite"));

  let headerHeight = $state(0);
  const list = new ListControl<UserSummary>({
    lines: () => shown,
    key: (u) => u.username,
    id: (u) => u.username,
    onopen: (u) => void goto(userHref(u.username)),
    rendered: (index) => lines.isDrawn(index),
    reveal: (index) => lines.scrollTo(index)
  });
  // a buddy list can run to hundreds; the window draws those in view
  const lines: ListWindow<UserSummary> = new ListWindow(
    () => shown,
    (u) => u.username,
    () => ({
      estimateSize: () => 2.5 * rem.px,
      overscan: 10,
      place: "users.buddies",
      scrollMargin: headerHeight,
      scrollPaddingStart: headerHeight,
      sizes: rem.px,
      onchange: () => list.scrolled()
    })
  );

  let messaging = $state(false);
  let adding = $state("");
  let busy = $state(false);
  const add = async () => {
    // a name is used as written, spaces at its ends too, which make it another user's
    const name = adding;
    if (!name.trim() || busy) return;
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

  /** Takes `name` off the list, with an Undo that puts them back as they were */
  const drop = async (name: string, notifyPresence: boolean) => {
    const outcome = await setBuddy(name, false);
    if (!outcome.ok) return toast(outcome.error, "error");
    toast(`Removed ${name} from ${BUDDY.plural.toLowerCase()}`, "info", {
      action: { label: "Undo", run: () => void change(() => befriend(name, { notifyPresence })) }
    });
  };

  /** the presence and since when it holds: the time of day for today, with the date before */
  const since = (u: UserSummary) =>
    u.presence && u.presenceAt
      ? `${u.presence} since ${chatTime(u.presenceAt, clock.now)}`
      : "Unknown";
</script>

<div class="flex flex-wrap items-center gap-inline">
  {#if write}
    <form
      class="flex min-w-0 items-center gap-inline"
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
      <Button
        type="submit"
        size="sm"
        variant="outline"
        class="shrink-0"
        disabled={!adding.trim() || busy}><UserPlusIcon />{BUDDY.add}</Button>
    </form>
  {/if}
  {#if shown.length}
    <span class="ml-auto text-sm text-fg-muted"
      >{formatCount(online.length)} of {plural(shown.length, "buddy", "buddies")} online</span>
    {#if write}
      <Button
        size="sm"
        variant="outline"
        disabled={!online.length}
        onclick={() => (messaging = true)}><MessageSquareIcon />Message online</Button>
    {/if}
  {/if}
</div>

{#if write}
  <MessageAll
    bind:open={messaging}
    usernames={online}
    title="Message buddies online"
    description={`To the ${plural(online.length, "buddy", "buddies")} online now`} />
{/if}

<LoadError loader={{ error, load }} />

{#if !loaded}
  <Skeleton class="h-40" />
{:else if !shown.length}
  <EmptyState
    icon={UsersIcon}
    title="No buddies yet"
    description="See when buddies are online and share folders with them alone" />
{:else}
  <div
    class="max-h-[max(20rem,calc(var(--content-h)-12rem))] overflow-auto rounded-card border bg-surface text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label={BUDDY.plural}
    aria-activedescendant={list.activeId}
    aria-rowcount={shown.length + 1}
    tabindex="0"
    {@attach lines.attach}
    {@attach list.attach}>
    <div class="sticky top-0 z-10" bind:offsetHeight={headerHeight}>
      <GridHeader {grid} rowindex={1} class="h-9 border-b bg-surface-muted text-fg-muted" />
    </div>
    <div
      role="rowgroup"
      class="relative"
      style:height={`${lines.total}px`}
      {@attach freezesWhileUsed}>
      {#each lines.drawn as { line: u, index, start } (u.username)}
        {@const buddy = u.buddy ?? { notifyPresence: false }}
        <div
          role="row"
          {...list.line(u, index)}
          aria-rowindex={index + 2}
          data-index={index}
          {@attach lines.measure}
          class="absolute inset-x-0 top-0 grid items-center list-row py-1.5"
          style={grid.row}
          style:transform={`translateY(${start - headerHeight}px)`}>
          <span role="gridcell" class="flex min-w-0 items-center gap-2 px-2">
            <Avatar
              name={u.username}
              pictureUrl={pictureOf(u.username, u)}
              class="size-7 text-xs" />
            <Username name={u.username} />
            {#if u.block}<Badge tone="danger">blocked</Badge>{/if}
          </span>
          <Tooltip text={u.presenceAt ? dateTime(u.presenceAt) : undefined} dense>
            {#snippet child({ props })}
              <span
                {...props}
                role="gridcell"
                class="flex min-w-0 items-center gap-1.5 px-2 text-xs">
                <Presence presence={u.presence} worded />{since(u)}
                <Flag code={u.countryCode} />
              </span>
            {/snippet}
          </Tooltip>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums"
            >{u.statistics ? formatSpeed(u.statistics.averageSpeed) : ""}</span>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums"
            >{u.statistics ? formatCount(u.statistics.fileCount) : ""}</span>
          <Tooltip text={u.notes ?? undefined} clipped>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate px-2 text-xs text-fg-muted"
                >{u.notes ?? ""}</span>
            {/snippet}
          </Tooltip>
          <span role="gridcell" class="px-2">
            <Switch
              aria-label={`${BUDDY.notify}: ${u.username}`}
              checked={buddy.notifyPresence}
              disabled={!write}
              onCheckedChange={(on) =>
                void change(() => befriend(u.username, { notifyPresence: on }))} />
          </span>
          <span role="gridcell" class="flex justify-end px-1">
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
                    onclick={() => void drop(u.username, buddy.notifyPresence)}
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
