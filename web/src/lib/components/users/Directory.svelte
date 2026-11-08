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
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
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
  import { Grid, type GridColumn } from "#lib/lists/table.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { canManage, configBlocked, reach, setBuddy, userGroups } from "#lib/users/actions.js";
  import { blocking, toggleBlock, unblockMany } from "#lib/users/block.svelte.js";
  import { pictureOf } from "#lib/users/profile.svelte.js";
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
  /** how many of a user's tags a row shows; the rest are on the profile */
  const TAGS = 3;
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
  let buddies = $state(false);
  // svelte-ignore state_referenced_locally
  let hasTransfers = $state(withTransfers);
  let presence = $state<Schemas["UserPresence"] | "">("");
  let group = $state("");
  // svelte-ignore state_referenced_locally
  let tag = $state(startTag ?? "");
  /** the daemon sorts the users, by the column the header sets */
  const sort = $derived(view.sort ?? { id: "lastSeen", dir: "desc" as const });
  let assigning = $state(false);

  const query = $derived({
    q: q || undefined,
    blocked: blocked || undefined,
    buddy: buddies || undefined,
    hasTransfers: hasTransfers || undefined,
    presence: presence || undefined,
    group: group || undefined,
    tag: tag || undefined,
    sort: sort.id as Schemas["UserSort"],
    order: sort.dir
  });

  const load = async () => {
    const turn = rows.reading();
    const ask = query;
    const want = wanted;
    const found: UserSummary[] = [];
    let count = 0;
    for (let offset = 0; offset < want;) {
      const limit = Math.min(500, want - offset);
      const outcome = await call(() =>
        api.GET("/api/v0/users", { params: { query: { ...ask, offset, limit } } })
      );
      if (!outcome.ok) {
        if (!rows.abandon(turn)) return;
        error = outcome.error;
        loaded = true;
        return;
      }
      // a newer load makes this one's answer moot
      if (!rows.latest(turn)) return;
      found.push(...outcome.data.users);
      count = outcome.data.total;
      offset += limit;
      if (outcome.data.users.length < limit) break;
    }
    rows.reconcile(found, turn);
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
  const admin = $derived(session.can("Administrator"));
  /** what is typed in the search box, which sets `q` a moment after typing stops */
  let typed = $state("");
  let box = $state<HTMLElement | null>(null);
  const refocus = () => box?.focus();
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
    ...when(buddies, "buddies", BUDDY.plural, () => (buddies = false)),
    ...when(hasTransfers, "hasTransfers", "Has transfers", () => (hasTransfers = false)),
    ...when(presence, "presence", presence, () => (presence = "")),
    ...when(group, "group", `Group ${group}`, () => (group = "")),
    ...when(tag, "tag", `Tagged ${tag}`, () => (tag = ""))
  ]);

  const refilter = (change: () => void) => {
    change();
    wanted = PAGE;
  };
  const search = (text: string) => {
    if (text !== q) refilter(() => (q = text));
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
    { id: "group", label: "Group", width: "8.5rem" },
    { id: "tags", label: "Tags", width: "8rem" },
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
      wanted = PAGE;
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
      {#each [{ label: "Blocked", on: blocked, set: (v: boolean) => (blocked = v) }, { label: BUDDY.plural, on: buddies, set: (v: boolean) => (buddies = v) }, { label: "Has transfers", on: hasTransfers, set: (v: boolean) => (hasTransfers = v) }] as chip (chip.label)}
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
    description="Users you transfer with, message or browse appear here" />
{:else}
  <div
    bind:this={box}
    class="overflow-x-auto rounded-card border bg-surface text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Users"
    aria-multiselectable="true"
    aria-rowcount={total}
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}>
    <GridHeader {grid} all={list.allTick} class="h-9 border-b bg-surface-muted text-fg-muted" />
    <div role="rowgroup" {@attach freezesWhileUsed}>
      {#each shown as u, i (u.username)}
        <div
          role="row"
          {...list.line(u, i)}
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
              onclick={(e) => {
                e.preventDefault();
                list.tick(u.username, e.shiftKey);
              }}
              onCheckedChange={(on) =>
                on !== list.selected.has(u.username) && list.toggle(u.username)} />
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
            <Presence presence={u.presence} worded />{u.presence ?? "Unknown"}
          </span>
          {#if viewport.xxl}
            <span role="gridcell" class="min-w-0 px-2"><GroupBadge group={u.group} /></span>
          {/if}
          <span role="gridcell" class="flex min-w-0 flex-wrap gap-1 px-2">
            {#each u.tags.slice(0, TAGS) as t (t)}
              <Tooltip text={`Show only users tagged ${t}`}>
                {#snippet child({ props })}
                  <button
                    {...props}
                    type="button"
                    class="max-w-full truncate rounded-badge border px-1 text-xs text-fg-muted hover:bg-hover"
                    onclick={() => refilter(() => (tag = t))}>{t}</button>
                {/snippet}
              </Tooltip>
            {/each}
            {#if u.tags.length > TAGS}
              <Tooltip text={u.tags.slice(TAGS).join("\n")}>
                {#snippet child({ props })}
                  <a
                    {...props}
                    href={userHref(u.username)}
                    class="rounded-badge px-1 text-xs text-fg-muted hover:bg-hover"
                    >+{formatCount(u.tags.length - TAGS)}</a>
                {/snippet}
              </Tooltip>
            {/if}
          </span>
          <Tooltip text={u.lastSeenAt ? dateTime(u.lastSeenAt) : undefined} dense>
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
              <Tooltip text={action.label}>
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
              <Tooltip text={u.buddy ? BUDDY.remove : BUDDY.add}>
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
              <!-- a disabled button takes no focus, so the reason is on a span around it -->
              <Tooltip text={byConfig ? CONFIG_BLOCKED : u.block ? "Unblock" : "Block"}>
                {#snippet child({ props })}
                  <span {...byConfig ? props : {}} class="inline-flex">
                    <Button
                      {...byConfig ? {} : props}
                      variant="ghost"
                      size="icon-sm"
                      aria-label={u.block ? "Unblock" : "Block"}
                      disabled={byConfig}
                      onclick={() => toggleBlock(u.username, !!u.block)}>
                      {#if u.block}<ShieldCheckIcon />{:else}<BanIcon />{/if}
                    </Button>
                  </span>
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
  {#if order.length < total}
    <Button variant="outline" class="self-center" onclick={() => (wanted = order.length + PAGE)}
      >Load more</Button>
  {/if}
{/if}

{#if list.selected.size}
  <div class="selection-bar" role="toolbar" aria-label="Selected users">
    <span class="text-sm">{formatCount(picked.length)} selected</span>
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
