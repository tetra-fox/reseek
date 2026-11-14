<script lang="ts" module>
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the directory's sort and column widths, kept in this browser */
  const view = gridView(
    "reseek.users.directory",
    ["username", "lastSeen", "uploads", "downloads"],
    { id: "lastSeen", dir: "desc" }
  );
</script>

<script lang="ts">
  import { goto } from "$app/navigation";
  import BanIcon from "@lucide/svelte/icons/ban";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";
  import UsersIcon from "@lucide/svelte/icons/users";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { api, type Schemas } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import * as Filter from "#lib/components/ui/filter/filter.js";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Pagination from "#lib/components/ui/Pagination.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, badgeCount, dateTime, formatCount, plural } from "#lib/format.js";
  import { type ActiveFilter, typedFilter } from "#lib/lists/filtered.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { pageCount, type Place } from "#lib/lists/pages.js";
  import { Grid, type GridColumn } from "#lib/lists/table.svelte.js";
  import { wakeOnPointer } from "#lib/lists/wake.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { content } from "#lib/shell/content.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { footBars } from "#lib/ui/footbars.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { canManage, configBlocked, reach, setBuddy, setGroups } from "#lib/users/actions.js";
  import { blocking, toggleBlock, unblockMany } from "#lib/users/block.svelte.js";
  import { DIRECTORY_PAGES, type DirectoryAddress } from "#lib/users/directory.js";
  import { BUDDIES, chosenGroups, queueOf, withGroup, withoutGroup } from "#lib/users/groups.js";
  import { pictureOf } from "#lib/users/profile.svelte.js";
  import { type UserSummary, users as usersStore } from "#lib/users/users.svelte.js";
  import { BUDDY, CONFIG_BLOCKED } from "#lib/wording.js";

  import Avatar from "./Avatar.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import GroupDot from "./GroupDot.svelte";
  import Presence from "./Presence.svelte";
  import UserMenuItems from "./UserMenuItems.svelte";
  import Username from "./Username.svelte";
  import UserSearch from "./UserSearch.svelte";

  type Props = {
    /** the filters and the page shown, from the address */
    address: DirectoryAddress;
    /** writes another of them into the address */
    onaddress: (next: DirectoryAddress) => void;
  };

  let { address, onaddress }: Props = $props();

  /**
   * an address asked for that the page has not taken yet: a filter's change and its return to
   * page 1 then read once, together, rather than the old page under the new filter first
   */
  let asked = $state.raw<DirectoryAddress | null>(null);
  // the address moved, by this list or by a link: it is the place again
  $effect.pre(() => {
    void address;
    untrack(() => (asked = null));
  });
  // apart, so a new address with the same values reads nothing again
  const at = $derived(asked ?? address);
  const page = $derived(at.page);
  const size = $derived(at.size);
  const q = $derived(at.q);
  const blocked = $derived(at.blocked);
  const buddies = $derived(at.buddies);
  const presence = $derived(at.presence);
  const group = $derived(at.group);
  const hasTransfers = $derived(at.hasTransfers);
  const place = $derived({ page, size });
  const moveTo = (next: Partial<DirectoryAddress>) => {
    asked = { q, blocked, buddies, presence, group, hasTransfers, page, size, ...next };
    onaddress(asked);
  };

  /** how many of a user's groups a row shows; the rest are on the profile */
  const SHOWN_GROUPS = 3;

  const rows = new Keyed<UserSummary>((u) => u.username);
  /** the users whose rows a mouse or a pen has come to, which then mount their tooltips */
  const awake = new SvelteSet<string>();
  /** the usernames in the order the daemon sorted them */
  let order = $state<string[]>([]);
  let total = $state(0);
  let loaded = $state(false);
  let error = $state("");
  /** the daemon sorts the users, by the column the header sets */
  const sort = $derived(view.sort ?? { id: "lastSeen", dir: "desc" as const });

  const query = $derived({
    q: q || undefined,
    blocked: blocked || undefined,
    buddy: buddies || undefined,
    hasTransfers: hasTransfers || undefined,
    presence: presence || undefined,
    group: group || undefined,
    sort: sort.id as Schemas["UserSort"],
    order: sort.dir
  });

  /** Reads the page shown; one past the end, as after a delete or a filter, goes to the last */
  const load = async () => {
    const turn = rows.reading();
    const read = place;
    const offset = (read.page - 1) * read.size;
    const outcome = await call(() =>
      api.GET("/api/v0/users", { params: { query: { ...query, offset, limit: read.size } } })
    );
    if (!outcome.ok) {
      if (!rows.abandon(turn)) return;
      error = outcome.error;
      loaded = true;
      return;
    }
    // a newer load makes this one's answer moot
    if (!rows.latest(turn)) return;
    const { users, total: count } = outcome.data;
    if (read.page > 1 && offset >= count) {
      rows.abandon(turn);
      return moveTo({ page: pageCount(count, read.size), size: read.size });
    }
    rows.reconcile(users, turn);
    order = users.map((u) => u.username);
    total = count;
    error = "";
    loaded = true;
  };

  keepCurrent(
    () => void load(),
    () => "users",
    rows.freeze
  );
  $effect(() => {
    void query;
    void page;
    void size;
    untrack(() => void load());
  });
  /** Shows another page from its top */
  const goPlace = (next: Place) => {
    moveTo(next);
    content.scroller?.scrollTo({ top: 0 });
  };
  // a user written anywhere is patched where it is; new users come with the next load
  $effect(() =>
    live.on("users.updated", (summary) => {
      if (rows.get(summary.username)) rows.upsert(summary);
    })
  );
  $effect(() => live.on("users.removed", ({ username }) => rows.remove(username)));

  // every group with members, for the filter and the selection's group actions
  const groupList = new Loader(() => call(() => api.GET("/api/v0/groups")));
  void groupList.load();
  $effect(() => live.on("application.options", () => void groupList.load()));
  const groupNames = $derived.by(() => {
    const names = groupList.data?.groups.map((g) => g.name) ?? [];
    return group && !names.includes(group) ? [...names, group] : names;
  });
  /** the groups a selection can be put in: the user-defined groups of the configuration */
  const joinable = $derived(groupList.data?.groups.filter((g) => !g.builtin && g.defined) ?? []);

  /** the loaded users, buddies first within the sort */
  const shown = $derived.by(() => {
    const users = order.flatMap((name) => rows.get(name) ?? []);
    return [...users.filter((u) => u.buddy), ...users.filter((u) => !u.buddy)];
  });

  const list = new ListControl<UserSummary>({
    lines: () => shown,
    key: (u) => u.username,
    id: (u) => u.username,
    onopen: (u) => void goto(userHref(u.username))
  });

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
  const write = $derived(session.can("ReadWrite"));
  /** the groups we put a selected user in, which the selection can be taken out of */
  const leavable = $derived(
    [...new Set(picked.flatMap((name) => chosenGroups(known(name)?.groups ?? [])))].toSorted()
  );
  /**
   * Puts each selected user in the groups `change` makes of theirs, with an Undo that puts back
   * what each had
   */
  const regroup = async (change: (held: string[]) => string[], done: string) => {
    const before = new Map<string, string[]>();
    const failed: string[] = [];
    for (const name of picked) {
      const user = known(name);
      if (!user) continue;
      const held = chosenGroups(user.groups);
      const next = change(held);
      if (next.length === held.length && next.every((g) => held.includes(g))) continue;
      const outcome = await setGroups(name, next);
      if (outcome.ok) before.set(name, held);
      else failed.push(`${name}: ${outcome.error}`);
    }
    if (failed.length) toast(failed.join("; "), "error");
    if (!before.size) return;
    toast(`${plural(before.size, "user", "users")} ${done}`, "info", {
      action: {
        label: "Undo",
        run: async () => {
          const undone: string[] = [];
          for (const [name, held] of before) {
            const outcome = await setGroups(name, held);
            if (!outcome.ok) undone.push(`${name}: ${outcome.error}`);
          }
          if (undone.length) toast(`Could not undo: ${undone.join("; ")}`, "error");
        }
      }
    });
  };
  /** what is typed in the search box, which sets `q` a moment after typing stops */
  // svelte-ignore state_referenced_locally
  let typed = $state(q);
  // a search the address brings, by a link or Back, is the box's text
  $effect.pre(() => {
    const text = address.q;
    untrack(() => {
      if (typed.trim() !== text) typed = text;
    });
  });
  let box = $state<HTMLElement | null>(null);
  const refocus = () => box?.focus();
  const USERS = ["user", "users"] as const;
  /** a filter that is on while `on` holds, which `off` turns off */
  const when = (
    on: unknown,
    id: string,
    label: string,
    off: Partial<DirectoryAddress>
  ): ActiveFilter[] => (on ? [{ id, label, clear: () => refilter(off) }] : []);
  const filters = $derived<ActiveFilter[]>([
    ...typedFilter(q, () => {
      typed = "";
      refilter({ q: "" });
    }),
    ...when(blocked, "blocked", "Blocked", { blocked: false }),
    ...when(buddies, "buddies", BUDDY.plural, { buddies: false }),
    ...when(hasTransfers, "hasTransfers", "Has transfers", { hasTransfers: false }),
    ...when(presence, "presence", presence, { presence: "" }),
    ...when(group, "group", `Group ${group}`, { group: "" })
  ]);

  /** A filter changed, in the address: the list starts again from its first page */
  const refilter = (next: Partial<DirectoryAddress>) => moveTo({ ...next, page: 1 });
  const search = (text: string) => {
    if (text !== q) refilter({ q: text });
  };

  const toggleBuddy = async (u: UserSummary) => {
    const outcome = await setBuddy(u.username, !u.buddy);
    if (!outcome.ok) toast(outcome.error, "error");
  };
  const unblockAll = (names: string[]) => unblockMany(names.filter((name) => known(name)?.block));

  // below a very wide screen the group column is left out, so the table fits beside the sidebar
  // on a 1280px screen, a scroll bar's width to spare, without a sideways scroll. The group
  // shows on the user's card
  const COLUMNS: GridColumn<UserSummary>[] = [
    { id: "select", label: "Select", width: "2.5rem", quiet: true, fixed: true },
    { id: "username", label: "User", width: "9rem", grow: true, sortable: true },
    { id: "presence", label: "Presence", width: "5.5rem" },
    { id: "group", label: "Queue", width: "8.5rem" },
    { id: "groups", label: "Groups", width: "8rem" },
    { id: "lastSeen", label: "Last seen", width: "7rem", sortable: true, descFirst: true },
    {
      id: "uploads",
      label: "Uploads",
      width: "6rem",
      end: true,
      sortable: true,
      descFirst: true
    },
    {
      id: "downloads",
      label: "Downloads",
      width: "7rem",
      end: true,
      sortable: true,
      descFirst: true
    },
    { id: "unread", label: "Unread", width: "4.5rem", end: true },
    // all five of a row's buttons
    { id: "actions", label: "Actions", width: "9.5rem", quiet: true, fixed: true }
  ];
  const grid = new Grid<UserSummary>({
    columns: () => COLUMNS,
    data: () => shown,
    sort: () => sort,
    setSort: (next) => {
      view.sort = next;
      if (place.page !== 1) moveTo({ page: 1, size: place.size });
    },
    dragged: () => view.dragged,
    drag: (id, width) => (view.dragged[id] = width),
    shown: () => COLUMNS.flatMap((c) => (c.id !== "group" || viewport.xxl ? [c.id] : [])),
    presorted: true
  });
</script>

<div class="flex flex-wrap items-center gap-inline">
  <UserSearch onsearch={search} bind:typed />
  <Filter.Panel {filters}>
    <Filter.Group label="Show">
      {#each [{ label: "Blocked", on: blocked, set: (v: boolean) => ({ blocked: v }) }, { label: BUDDY.plural, on: buddies, set: (v: boolean) => ({ buddies: v }) }, { label: "Has transfers", on: hasTransfers, set: (v: boolean) => ({ hasTransfers: v }) }] as chip (chip.label)}
        <Filter.Chip pressed={chip.on} onclick={() => refilter(chip.set(!chip.on))}
          >{chip.label}</Filter.Chip>
      {/each}
      <Select
        class={["h-control-sm w-36", presence && Filter.FILTERING]}
        aria-label="Presence"
        bind:value={() => presence, (v) => refilter({ presence: v })}
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
        bind:value={() => group, (v) => refilter({ group: v })}
        options={[
          { value: "", label: "Any group" },
          ...groupNames.map((g) => ({ value: g, label: g }))
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
    description="Users you transfer with, message or browse appear here" />
{:else}
  <!-- one page of the users is in the grid; the rows count and place themselves among all of
       them, the header row first -->
  <div
    bind:this={box}
    class="overflow-x-auto rounded-card border bg-surface text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Users"
    aria-multiselectable="true"
    aria-rowcount={total + 1}
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}>
    <GridHeader
      {grid}
      all={list.allTick}
      rowindex={1}
      class="h-9 border-b bg-surface-muted text-fg-muted" />
    <div role="rowgroup" {@attach freezesWhileUsed(rows.freeze)}>
      {#each shown as u, i (u.username)}
        {@const listed = u.groups.filter((g) => g.name !== BUDDIES)}
        {@const queue = queueOf(u)}
        {@const dormant = !awake.has(u.username)}
        <div
          role="row"
          {...list.line(u, i)}
          aria-rowindex={(place.page - 1) * place.size + i + 2}
          onpointerenter={wakeOnPointer(() => awake.add(u.username))}
          class={[
            "grid items-center list-row py-1.5",
            u.block && "bg-danger-soft/40",
            rows.removed.has(u.username) && "opacity-40"
          ]}
          style={grid.row}>
          <span role="gridcell" class="flex px-2">
            <Checkbox
              aria-label={`Select ${u.username}`}
              checked={list.selected.has(u.username)}
              ontick={(range) => list.tick(u.username, range)} />
          </span>
          <span role="gridcell" class="flex min-w-0 items-center gap-2 px-2">
            <Avatar
              name={u.username}
              pictureUrl={pictureOf(u.username, u)}
              class="size-7 text-xs" />
            <Username name={u.username} />
            {#if u.buddy}<Badge tone="info">{BUDDY.badge}</Badge>{/if}
            {#if u.block}<Badge tone="danger">blocked</Badge>{/if}
          </span>
          <span role="gridcell" class="flex min-w-0 items-center gap-1.5 px-2 text-xs">
            <Presence presence={u.presence} worded {dormant} />{u.presence ?? "Unknown"}
          </span>
          {#if viewport.xxl}
            <span role="gridcell" class="min-w-0 px-2"
              >{#if queue}<GroupBadge group={queue} queue {dormant} />{/if}</span>
          {/if}
          <span role="gridcell" class="flex min-w-0 flex-wrap gap-1 px-2">
            {#each listed.slice(0, SHOWN_GROUPS) as g (g.name)}
              <button
                type="button"
                class="max-w-full rounded-badge hover:bg-hover"
                aria-label={`Show only group ${g.name}`}
                onclick={() => refilter({ group: g.name })}
                ><GroupBadge group={g} {dormant} /></button>
            {/each}
            {#if listed.length > SHOWN_GROUPS}
              <Tooltip
                text={listed
                  .slice(SHOWN_GROUPS)
                  .map((g) => g.name)
                  .join("\n")}
                {dormant}>
                {#snippet child({ props })}
                  <a
                    {...props}
                    href={userHref(u.username)}
                    class="rounded-badge px-1 text-xs text-fg-muted hover:bg-hover"
                    >+{formatCount(listed.length - SHOWN_GROUPS)}</a>
                {/snippet}
              </Tooltip>
            {/if}
          </span>
          <Tooltip text={u.lastSeenAt ? dateTime(u.lastSeenAt) : undefined} dense {dormant}>
            {#snippet child({ props })}
              <span {...props} role="gridcell" class="truncate px-2 text-xs">
                {u.lastSeenAt ? ago(u.lastSeenAt, clock.now) : "never"}
              </span>
            {/snippet}
          </Tooltip>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums"
            >{formatCount(u.figures.uploads.succeeded)}</span>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums"
            >{formatCount(u.figures.downloads.succeeded)}</span>
          <span role="gridcell" class="px-2 text-right tabular-nums">
            {#if u.unreadMessages}<Badge tone="danger">{badgeCount(u.unreadMessages)}</Badge>{/if}
          </span>
          <span role="gridcell" class="flex justify-end px-1">
            {#each reach(u.username, { only: ["message", "shares"] }) as action (action.id)}
              <Tooltip text={action.label} {dormant}>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    href={action.href}
                    aria-label={action.label}><action.icon /></Button>
                {/snippet}
              </Tooltip>
            {/each}
            {#if canManage(u.username)}
              <Tooltip text={u.buddy ? BUDDY.remove : BUDDY.add} {dormant}>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={u.buddy ? BUDDY.remove : BUDDY.add}
                    aria-pressed={!!u.buddy}
                    onclick={() => void toggleBuddy(u)}>
                    {#if u.buddy}<UserCheckIcon class="text-accent" />{:else}<UserPlusIcon />{/if}
                  </Button>
                {/snippet}
              </Tooltip>
              {@const byConfig = configBlocked(u.block)}
              <Tooltip text={byConfig ? CONFIG_BLOCKED : u.block ? "Unblock" : "Block"} {dormant}>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={u.block ? "Unblock" : "Block"}
                    aria-disabled={byConfig || undefined}
                    onclick={() => toggleBlock(u.username, !!u.block)}>
                    {#if u.block}<ShieldCheckIcon />{:else}<BanIcon />{/if}
                  </Button>
                {/snippet}
              </Tooltip>
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
  {#if total > DIRECTORY_PAGES.sizes[0]}
    <div class="flex flex-wrap items-center justify-center gap-x-4 gap-y-2">
      <Pagination
        page={place.page}
        pages={pageCount(total, place.size)}
        onpage={(page) => goPlace({ page, size: place.size })} />
      <!-- a new size goes to the page that holds the first row shown -->
      <Select
        class="h-control-sm w-32"
        aria-label="Users per page"
        bind:value={
          () => place.size,
          (size) => goPlace({ page: Math.floor(((place.page - 1) * place.size) / size) + 1, size })
        }
        options={DIRECTORY_PAGES.sizes.map((n) => ({ value: n, label: `${n} per page` }))} />
    </div>
  {/if}
{/if}

{#if list.selected.size}
  <div class="selection-bar" role="toolbar" aria-label="Selected users" {@attach footBars.attach}>
    <span class="text-sm">{formatCount(picked.length)} selected</span>
    {#if blockable.length}
      <Button size="sm" variant="outline" onclick={() => blocking.ask(blockable)}>Block</Button>
      <Button size="sm" variant="outline" onclick={() => void unblockAll(blockable)}
        >Unblock</Button>
    {/if}
    {#if write && joinable.length}
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button {...props} size="sm" variant="outline">Add to group<ChevronDownIcon /></Button>
          {/snippet}
        </Menu.Trigger>
        <Menu.Content align="start">
          {#each joinable as g (g.name)}
            <Menu.Item
              onSelect={() => void regroup((held) => withGroup(held, g.name), `added to ${g.name}`)}
              ><GroupDot color={g.color} />{g.name}</Menu.Item>
          {/each}
        </Menu.Content>
      </Menu.Root>
    {/if}
    {#if write && leavable.length}
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button {...props} size="sm" variant="outline"
              >Remove from group<ChevronDownIcon /></Button>
          {/snippet}
        </Menu.Trigger>
        <Menu.Content align="start">
          {#each leavable as name (name)}
            <Menu.Item
              onSelect={() =>
                void regroup((held) => withoutGroup(held, name), `removed from ${name}`)}
              >{name}</Menu.Item>
          {/each}
        </Menu.Content>
      </Menu.Root>
    {/if}
    <Button size="sm" variant="ghost" onclick={() => list.clear()}>Clear</Button>
  </div>
{/if}
