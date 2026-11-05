<script lang="ts">
  import { goto } from "$app/navigation";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import BanIcon from "@lucide/svelte/icons/ban";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";
  import UsersIcon from "@lucide/svelte/icons/users";
  import { untrack } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, dateTime, plural } from "#lib/format.js";
  import { type ActiveFilter, typedFilter } from "#lib/lists/filtered.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { canManage, configBlocked, pin, reach, userGroups } from "#lib/users/actions.js";
  import { blocking, toggleBlock, unblockMany } from "#lib/users/block.svelte.js";
  import { type UserSummary, users as usersStore } from "#lib/users/users.svelte.js";
  import { BUDDY, CONFIG_BLOCKED } from "#lib/wording.js";

  import Avatar from "./Avatar.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import GroupDialog from "./GroupDialog.svelte";
  import Presence from "./Presence.svelte";
  import UserMenuItems from "./UserMenuItems.svelte";
  import Username from "./Username.svelte";
  import UserSearch from "./UserSearch.svelte";

  type Props = {
    /** the tag the list starts filtered by, from the link that opened it */
    tag?: string | null;
    /** the list starts with the users we transferred with, from the link that opened it */
    withTransfers?: boolean;
  };

  let { tag: startTag = null, withTransfers = false }: Props = $props();

  const PAGE = 50;
  const BUILTIN_GROUPS = ["privileged", "default", "leechers"];

  const rows = new Keyed<UserSummary>((u) => u.username);
  /** the usernames in the order the daemon sorted them */
  let order = $state<string[]>([]);
  let total = $state(0);
  /** how many rows are loaded: a page, more for each "load more" */
  let wanted = $state(PAGE);
  let loaded = $state(false);
  let error = $state("");
  let q = $state("");
  let blocked = $state(false);
  let pinned = $state(false);
  // svelte-ignore state_referenced_locally
  let hasTransfers = $state(withTransfers);
  let presence = $state<Schemas["UserPresence"] | "">("");
  let group = $state("");
  // svelte-ignore state_referenced_locally
  let tag = $state(startTag ?? "");
  let sort = $state<{ by: Schemas["UserSort"]; order: Schemas["SortOrder"] }>({
    by: "lastSeen",
    order: "desc"
  });
  let assigning = $state(false);

  const query = $derived({
    q: q || undefined,
    blocked: blocked || undefined,
    pinned: pinned || undefined,
    hasTransfers: hasTransfers || undefined,
    presence: presence || undefined,
    group: group || undefined,
    tag: tag || undefined,
    sort: sort.by,
    order: sort.order
  });

  // a newer load makes an older one's answer moot
  let generation = 0;
  const load = async () => {
    const at = ++generation;
    const ask = query;
    const want = wanted;
    const found: UserSummary[] = [];
    let count = 0;
    for (let offset = 0; offset < want;) {
      const limit = Math.min(500, want - offset);
      const outcome = await call(() =>
        api.GET("/api/v0/users", { params: { query: { ...ask, offset, limit } } })
      );
      if (at !== generation) return;
      if (!outcome.ok) {
        error = outcome.error;
        loaded = true;
        return;
      }
      found.push(...outcome.data.users);
      count = outcome.data.total;
      offset += limit;
      if (outcome.data.users.length < limit) break;
    }
    rows.reconcile(found);
    order = found.map((u) => u.username);
    total = count;
    error = "";
    loaded = true;
  };

  keepCurrent(
    () => void load(),
    () => "users"
  );
  $effect(() => {
    void query;
    void wanted;
    untrack(() => void load());
  });
  // a user written anywhere is patched where it is; new users come with the next load
  $effect(() =>
    live.on("users.updated", (summary) => {
      if (rows.get(summary.username)) rows.upsert(summary);
    })
  );
  $effect(() => live.on("users.removed", ({ username }) => rows.remove(username)));

  /** the loaded users, pinned ones first within the sort */
  const shown = $derived.by(() => {
    const users = order.flatMap((name) => rows.get(name) ?? []);
    return [...users.filter((u) => u.pinned), ...users.filter((u) => !u.pinned)];
  });

  const list = new ListControl<UserSummary>({
    lines: () => shown,
    key: (u) => u.username,
    id: (u) => u.username,
    onopen: (u) => void goto(userHref(u.username))
  });

  /** the shown users selected, for the header's box */
  const chosen = $derived(shown.filter((u) => list.selected.has(u.username)));
  /** every user selected, those a filter now hides too, as the transfers grid counts them */
  const picked = $derived([...list.selected]);
  /** what is known of a user, shown here or loaded elsewhere */
  const known = (name: string) => rows.get(name) ?? usersStore.get(name);
  /**
   * the selected users the bar can block and unblock, leaving out a user the configuration blocks
   */
  const blockable = $derived(
    picked.filter((name) => canManage(name) && !configBlocked(known(name)?.block))
  );
  const admin = $derived(session.can("Administrator"));
  /** what is typed in the search box, which sets `q` a moment after typing stops */
  let typed = $state("");
  let grid = $state<HTMLElement | null>(null);
  const refocus = () => grid?.focus();
  const USERS = ["user", "users"] as const;
  /** a filter that is on while `on` holds, which `off` turns off */
  const when = (on: unknown, id: string, label: string, off: () => void): ActiveFilter[] =>
    on ? [{ id, label, clear: () => refilter(off) }] : [];
  const filters = $derived<ActiveFilter[]>([
    ...typedFilter(q, () =>
      refilter(() => {
        q = "";
        typed = "";
      })
    ),
    ...when(blocked, "blocked", "Blocked", () => (blocked = false)),
    ...when(pinned, "pinned", BUDDY.plural, () => (pinned = false)),
    ...when(hasTransfers, "hasTransfers", "Has transfers", () => (hasTransfers = false)),
    ...when(presence, "presence", presence, () => (presence = "")),
    ...when(group, "group", `Group ${group}`, () => (group = "")),
    ...when(tag, "tag", `Tagged ${tag}`, () => (tag = ""))
  ]);

  const sortBy = (by: Schemas["UserSort"]) => {
    const first = by === "username" ? "asc" : "desc";
    sort = {
      by,
      order: sort.by === by ? (sort.order === "asc" ? "desc" : "asc") : first
    };
    wanted = PAGE;
  };
  const refilter = (change: () => void) => {
    change();
    wanted = PAGE;
  };
  const search = (text: string) => {
    if (text !== q) refilter(() => (q = text));
  };

  const togglePin = async (u: UserSummary) => {
    const outcome = await pin(u.username, !u.pinned);
    if (!outcome.ok) toast(outcome.error, "error");
  };
  const unblockAll = (names: string[]) => unblockMany(names.filter((name) => known(name)?.block));

  // the columns; below a very wide screen the group column is left out, so the table fits
  // beside the sidebar without a sideways scroll. The group shows on the user's card
  // every row is a grid of its own, so the user's cell has no say in its width, and the last
  // track holds all five of a row's buttons
  const COLUMNS =
    "grid-cols-[2rem_minmax(11rem,1fr)_5rem_7rem_7rem_6rem_6rem_4rem_9rem] 2xl:grid-cols-[2rem_minmax(11rem,1fr)_5rem_8rem_7rem_7rem_6rem_6rem_4rem_9rem]";
</script>

{#snippet sortHeader(label: string, by: Schemas["UserSort"])}
  <button
    type="button"
    class="-my-2.5 flex items-center gap-1 truncate py-2.5 hover:text-fg"
    onclick={() => sortBy(by)}>
    {label}
    {#if sort.by === by}
      {#if sort.order === "asc"}<ArrowUpIcon class="size-3" />{:else}<ArrowDownIcon
          class="size-3" />{/if}
    {/if}
  </button>
{/snippet}

<div class="flex flex-wrap items-center gap-inline">
  <UserSearch onsearch={search} bind:typed />
  <Filter.Panel {filters}>
    <Filter.Group label="Show">
      {#each [{ label: "Blocked", on: blocked, set: (v: boolean) => (blocked = v) }, { label: BUDDY.plural, on: pinned, set: (v: boolean) => (pinned = v) }, { label: "Has transfers", on: hasTransfers, set: (v: boolean) => (hasTransfers = v) }] as chip (chip.label)}
        <Filter.Chip pressed={chip.on} onclick={() => refilter(() => chip.set(!chip.on))}
          >{chip.label}</Filter.Chip>
      {/each}
      <Select
        class={["h-control-sm w-36", presence && Filter.FILTERING]}
        aria-label="Presence"
        bind:value={() => presence, (v) => refilter(() => (presence = v))}
        options={[
          { value: "", label: "Any presence" },
          { value: "Online", label: "Online" },
          { value: "Away", label: "Away" },
          { value: "Offline", label: "Offline" }
        ]} />
      <!-- a group's name is the user's own, so the box takes the width of the one chosen -->
      <Select
        class={["h-control-sm w-fit max-w-48 min-w-32", group && Filter.FILTERING]}
        aria-label="Group"
        bind:value={() => group, (v) => refilter(() => (group = v))}
        options={[
          { value: "", label: "Any group" },
          ...[...BUILTIN_GROUPS, ...userGroups()].map((g) => ({ value: g, label: g }))
        ]} />
    </Filter.Group>
  </Filter.Panel>
  {#if !filters.length}
    <span class="ml-auto text-sm text-fg-muted">{plural(total, "user", "users")}</span>
  {/if}
</div>

{#if loaded}
  <Filter.Summary {filters} shown={total} total={null} noun={USERS} {refocus} />
{/if}

<LoadError loader={{ error, load }} />

{#if !loaded}
  <Skeleton class="h-64" />
{:else if !shown.length && filters.length}
  <Filter.Empty {filters} total={null} noun={USERS} {refocus} />
{:else if !shown.length}
  <EmptyState
    icon={UsersIcon}
    title="No users yet"
    description="Users from transfers, private messages and browsing are listed here." />
{:else}
  <div
    bind:this={grid}
    class="overflow-x-auto rounded-card border bg-surface text-sm shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Users"
    aria-multiselectable="true"
    aria-rowcount={total}
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}>
    <div
      role="row"
      class={[
        "grid h-9 min-w-max items-center gap-2 border-b bg-surface-muted px-2 text-xs font-medium text-fg-muted",
        COLUMNS
      ]}>
      <span role="columnheader">
        <Checkbox
          aria-label="Select every user shown"
          checked={chosen.length === shown.length}
          indeterminate={chosen.length > 0 && chosen.length < shown.length}
          onCheckedChange={(on) =>
            list.setAll(
              shown.map((u) => u.username),
              on
            )} />
      </span>
      <span role="columnheader">{@render sortHeader("User", "username")}</span>
      <span role="columnheader">Presence</span>
      <span role="columnheader" class="max-2xl:hidden">Group</span>
      <span role="columnheader">Tags</span>
      <span role="columnheader">{@render sortHeader("Last seen", "lastSeen")}</span>
      <span role="columnheader">{@render sortHeader("Uploads", "uploads")}</span>
      <span role="columnheader">{@render sortHeader("Downloads", "downloads")}</span>
      <span role="columnheader">Unread</span>
      <span role="columnheader" class="sr-only">Actions</span>
    </div>
    <div role="rowgroup" {@attach freezesWhileUsed}>
      {#each shown as u, i (u.username)}
        <div
          role="row"
          {...list.line(u, i)}
          class={[
            "grid min-w-max items-center gap-2 list-row px-2 py-1.5",
            COLUMNS,
            u.block && "bg-danger-soft/40",
            rows.removed.has(u.username) && "opacity-40"
          ]}>
          <span role="gridcell">
            <Checkbox
              aria-label={`Select ${u.username}`}
              checked={list.selected.has(u.username)}
              onclick={(e) => {
                e.preventDefault();
                list.tick(u.username, e.shiftKey);
              }}
              onCheckedChange={(on) =>
                on !== list.selected.has(u.username) && list.toggle(u.username)} />
          </span>
          <span role="gridcell" class="flex w-0 min-w-full items-center gap-2">
            <Avatar name={u.username} pictureUrl={u.info?.pictureUrl} class="size-7 text-xs" />
            <Username name={u.username} />
            {#if u.pinned}<Badge tone="info">{BUDDY.badge}</Badge>{/if}
            {#if u.block}<Badge tone="danger">blocked</Badge>{/if}
          </span>
          <span role="gridcell" class="flex items-center gap-1.5 text-xs">
            <Presence presence={u.presence} />{u.presence ?? "?"}
          </span>
          <span role="gridcell" class="max-2xl:hidden"><GroupBadge group={u.group} /></span>
          <span role="gridcell" class="flex min-w-0 flex-wrap gap-1">
            {#each u.tags as t (t)}
              <button
                type="button"
                class="rounded-badge border px-1 text-xs text-fg-muted hover:bg-hover"
                title={`Show only users tagged ${t}`}
                onclick={() => refilter(() => (tag = t))}>{t}</button>
            {/each}
          </span>
          <span role="gridcell" class="text-xs" title={u.lastSeenAt ? dateTime(u.lastSeenAt) : ""}>
            {u.lastSeenAt ? ago(u.lastSeenAt, clock.now) : "never"}
          </span>
          <span role="gridcell" class="text-right tabular-nums">{u.figures.uploads.succeeded}</span>
          <span role="gridcell" class="text-right tabular-nums"
            >{u.figures.downloads.succeeded}</span>
          <span role="gridcell" class="text-right tabular-nums">
            {#if u.unreadMessages}<Badge tone="danger">{u.unreadMessages}</Badge>{/if}
          </span>
          <span role="gridcell" class="flex justify-end">
            {#each reach(u.username, { only: ["message", "shares"] }) as action (action.id)}
              <Button
                variant="ghost"
                size="icon-sm"
                href={action.href}
                aria-label={action.label}
                title={action.label}><action.icon /></Button>
            {/each}
            {#if canManage(u.username)}
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={u.pinned ? BUDDY.remove : BUDDY.add}
                title={u.pinned ? BUDDY.remove : BUDDY.add}
                aria-pressed={u.pinned}
                onclick={() => void togglePin(u)}>
                {#if u.pinned}<UserCheckIcon class="text-accent" />{:else}<UserPlusIcon />{/if}
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={u.block ? "Unblock" : "Block"}
                disabled={configBlocked(u.block)}
                title={configBlocked(u.block) ? CONFIG_BLOCKED : undefined}
                onclick={() => toggleBlock(u.username, !!u.block)}>
                {#if u.block}<ShieldCheckIcon />{:else}<BanIcon />{/if}
              </Button>
            {/if}
            <Menu.Root>
              <Menu.Trigger>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`More for ${u.username}`}>
                    <EllipsisIcon />
                  </Button>
                {/snippet}
              </Menu.Trigger>
              <Menu.Content align="end">
                <UserMenuItems name={u.username} only={["profile", "search"]} />
                <Menu.Item onSelect={() => void copyText(u.username)}
                  ><CopyIcon />Copy username</Menu.Item>
              </Menu.Content>
            </Menu.Root>
          </span>
        </div>
      {/each}
    </div>
  </div>
  {#if order.length < total}
    <Button variant="outline" class="self-center" onclick={() => (wanted = order.length + PAGE)}
      >Load more</Button>
  {/if}
{/if}

{#if list.selected.size}
  <div class="selection-bar" role="toolbar" aria-label="Selected users">
    <span class="text-sm">{picked.length} selected</span>
    {#if blockable.length}
      <Button size="sm" variant="outline" onclick={() => blocking.ask(blockable)}>Block</Button>
      <Button size="sm" variant="outline" onclick={() => void unblockAll(blockable)}
        >Unblock</Button>
    {/if}
    {#if admin}
      <Button size="sm" variant="outline" onclick={() => (assigning = true)}>Assign group</Button>
    {/if}
    <Button size="sm" variant="ghost" onclick={() => list.clear()}>Clear</Button>
  </div>
{/if}

<GroupDialog bind:open={assigning} usernames={picked} />
