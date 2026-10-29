<script lang="ts">
  import { goto } from "$app/navigation";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import BanIcon from "@lucide/svelte/icons/ban";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import SearchIcon from "@lucide/svelte/icons/search";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";
  import UsersIcon from "@lucide/svelte/icons/users";
  import XIcon from "@lucide/svelte/icons/x";
  import { untrack } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import { clock } from "#lib/clock.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { ago, dateTime, plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { Keyed } from "#lib/keyed.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { type UserSummary, users as usersStore } from "#lib/users.svelte.js";
  import { BUDDY } from "#lib/wording.js";

  import { links, pin, userGroups } from "./actions";
  import Avatar from "./Avatar.svelte";
  import { blocking, unblock, unblockMany } from "./block.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import GroupDialog from "./GroupDialog.svelte";
  import Presence from "./Presence.svelte";
  import Username from "./Username.svelte";

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
  let typed = $state("");
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

  $effect(() => {
    const value = typed.trim();
    const timer = setTimeout(() => {
      if (value === q) return;
      q = value;
      wanted = PAGE;
    }, 200);
    return () => clearTimeout(timer);
  });

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
  const write = $derived(session.can("ReadWrite"));
  const admin = $derived(session.can("Administrator"));
  const filtered = $derived(!!(q || blocked || pinned || hasTransfers || presence || group || tag));

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

  const togglePin = async (u: UserSummary) => {
    const outcome = await pin(u.username, !u.pinned);
    if (!outcome.ok) toast(outcome.error, "error");
  };
  const unblockAll = (names: string[]) =>
    unblockMany(names.filter((name) => (rows.get(name) ?? usersStore.get(name))?.block));

  // the columns; below a very wide screen the group column is left out, so the table fits
  // beside the sidebar without a sideways scroll. The group shows on the user's card
  const COLUMNS =
    "grid-cols-[2rem_minmax(11rem,1fr)_5rem_7rem_7rem_6rem_6rem_4rem_7rem] 2xl:grid-cols-[2rem_minmax(11rem,1fr)_5rem_8rem_7rem_7rem_6rem_6rem_4rem_7rem]";
</script>

{#snippet sortHeader(label: string, by: Schemas["UserSort"])}
  <button type="button" class="flex items-center gap-1 truncate" onclick={() => sortBy(by)}>
    {label}
    {#if sort.by === by}
      {#if sort.order === "asc"}<ArrowUpIcon class="size-3" />{:else}<ArrowDownIcon
          class="size-3" />{/if}
    {/if}
  </button>
{/snippet}

<div class="flex flex-wrap items-center gap-inline">
  <Input
    icon={SearchIcon}
    wrapperClass="w-full max-w-xs"
    type="search"
    placeholder="Username, tag or note"
    aria-label="Search users"
    bind:value={typed} />
  <div class="flex flex-wrap items-center gap-1" role="group" aria-label="Filters">
    {#each [{ label: "Blocked", on: blocked, set: (v: boolean) => (blocked = v) }, { label: BUDDY.plural, on: pinned, set: (v: boolean) => (pinned = v) }, { label: "Has transfers", on: hasTransfers, set: (v: boolean) => (hasTransfers = v) }] as chip (chip.label)}
      <Button
        size="sm"
        variant={chip.on ? "secondary" : "outline"}
        aria-pressed={chip.on}
        onclick={() => refilter(() => chip.set(!chip.on))}>{chip.label}</Button>
    {/each}
    <Select
      class="h-control-sm w-36"
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
      class="h-control-sm w-fit max-w-48 min-w-32"
      aria-label="Group"
      bind:value={() => group, (v) => refilter(() => (group = v))}
      options={[
        { value: "", label: "Any group" },
        ...[...BUILTIN_GROUPS, ...userGroups()].map((g) => ({ value: g, label: g }))
      ]} />
    {#if tag}
      <Badge tone="accent">
        tag: {tag}
        <button type="button" aria-label="Clear the tag" onclick={() => refilter(() => (tag = ""))}>
          <XIcon />
        </button>
      </Badge>
    {/if}
  </div>
  <span class="ml-auto text-sm text-fg-muted">{plural(total, "user", "users")}</span>
</div>

<LoadError loader={{ error, load }} />

{#if !loaded}
  <Skeleton class="h-64" />
{:else if !shown.length}
  <EmptyState
    icon={UsersIcon}
    title={filtered ? "No users match" : "No users yet"}
    description={filtered
      ? undefined
      : "Users you download from, upload to, message or browse appear here."} />
{:else}
  <div
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
        {@const to = links(u.username)}
        <div
          role="row"
          {...list.line(u, i)}
          class={[
            "grid min-w-max items-center gap-2 list-row px-2 py-1.5 hover:bg-hover",
            COLUMNS,
            u.block && "bg-danger-soft/40",
            list.selected.has(u.username) && "bg-accent-soft/60",
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
          <span role="gridcell" class="flex min-w-0 items-center gap-2">
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
            {#if write}
              <Button variant="ghost" size="icon-sm" href={to.message} aria-label="Message">
                <MessageSquareIcon />
              </Button>
            {/if}
            <Button variant="ghost" size="icon-sm" href={to.browse} aria-label="Browse shares">
              <FolderOpenIcon />
            </Button>
            {#if write}
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={u.pinned ? BUDDY.remove : BUDDY.add}
                title={u.pinned ? BUDDY.remove : BUDDY.add}
                aria-pressed={u.pinned}
                onclick={() => void togglePin(u)}>
                {#if u.pinned}<UserCheckIcon class="text-accent" />{:else}<UserPlusIcon />{/if}
              </Button>
            {/if}
            {#if write}
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={u.block ? "Unblock" : "Block"}
                onclick={() => (u.block ? void unblock(u.username) : blocking.ask(u.username))}>
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
                <Menu.Item onSelect={() => goto(userHref(u.username))}>View profile</Menu.Item>
                <Menu.Item onSelect={() => void copyText(u.username)}>Copy username</Menu.Item>
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
    {#if write}
      <Button size="sm" variant="outline" onclick={() => blocking.ask(picked)}>Block</Button>
      <Button size="sm" variant="outline" onclick={() => void unblockAll(picked)}>Unblock</Button>
    {/if}
    {#if admin}
      <Button size="sm" variant="outline" onclick={() => (assigning = true)}>Assign group</Button>
    {/if}
    <Button size="sm" variant="ghost" onclick={() => list.clear()}>Clear</Button>
  </div>
{/if}

<GroupDialog bind:open={assigning} usernames={picked} />
