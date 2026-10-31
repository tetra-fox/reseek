<script lang="ts">
  import { goto } from "$app/navigation";
  import BellOffIcon from "@lucide/svelte/icons/bell-off";
  import CheckCheckIcon from "@lucide/svelte/icons/check-check";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import HashIcon from "@lucide/svelte/icons/hash";
  import MessageSquarePlusIcon from "@lucide/svelte/icons/message-square-plus";
  import UsersRoundIcon from "@lucide/svelte/icons/users-round";
  import { SvelteSet } from "svelte/reactivity";

  import { chat, paneKey, paneOf } from "#lib/chat/chat.svelte.js";
  import { actionOf } from "#lib/chat/messages/rich.js";
  import type { Target } from "#lib/chat/pane.svelte.js";
  import { sortedConversations, sortedRooms } from "#lib/chat/rooms/order.js";
  import type { SearchKind } from "#lib/chat/search/match.js";
  import SearchBox from "#lib/components/chat/search/SearchBox.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Avatar from "#lib/components/users/Avatar.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, plural } from "#lib/format.js";
  import { freezesWhileUsed, heldOrder, interaction } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { chatHref, roomHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { disclose } from "#lib/ui/motion.js";
  import { nameColor } from "#lib/users/colors.js";
  import { users } from "#lib/users/users.svelte.js";

  import ChatMenuItems from "./ChatMenuItems.svelte";

  type Props = {
    /** the pane on screen, by pane key */
    current: string | null;
    /** the search the address holds, which the search box shows */
    q?: string;
    /** the messages the address searches */
    kind?: SearchKind;
    onjoin: (name?: string) => void;
    onnew: (username?: string) => void;
    /** the search box, for Ctrl/Cmd+K */
    search?: HTMLInputElement | null;
  };

  let { current, q = "", kind = "all", onjoin, onnew, search = $bindable(null) }: Props = $props();

  type Line =
    | { kind: "header"; key: string; title: string; ids: string[] }
    | { kind: "row"; key: string; target: Target; ids: string[] };

  const folded = new SvelteSet<string>();
  /** the row whose menu is open, from its button or a right click */
  let menuFor = $state<string | null>(null);

  const rooms = $derived(sortedRooms());
  const conversations = $derived(sortedConversations());

  // the list holds still while the pointer is on it, a text is selected in it or a row's menu
  // is open, so a row never moves away from a click
  const hold = heldOrder<Line>((l) => l.key);
  $effect(() => (menuFor ? interaction.hold() : undefined));
  const lines = $derived.by(() => {
    const section = (key: string, title: string, targets: Target[]): Line[] => {
      const ids = targets.map((t) => paneKey(t.kind, t.name));
      return [
        { kind: "header", key, title, ids },
        ...(folded.has(key)
          ? []
          : targets.map((target): Line => ({
              kind: "row",
              key: paneKey(target.kind, target.name),
              target,
              ids
            })))
      ];
    };
    return hold([
      ...section(
        "h:rooms",
        "Rooms",
        rooms.map((r) => ({ kind: "room", name: r.name }))
      ),
      ...section(
        "h:conversations",
        "Conversations",
        conversations.map((c) => ({ kind: "user", name: c.username }))
      )
    ]);
  });
  // focus stays on the list, so the arrows go on from the chat just opened
  const open = (target: Target) =>
    void goto(target.kind === "user" ? chatHref(target.name) : roomHref(target.name), {
      reset: false
    });

  const list = new ListControl<Line>({
    lines: () => lines,
    key: (l) => l.key,
    id: (l) => (l.kind === "row" ? l.key : null),
    group: (l) => l.ids,
    onopen: (l) => {
      if (l.kind === "row") open(l.target);
    },
    openOnClick: true
  });

  // every chat selected, those a folded section hides too
  const chosen = $derived([...list.selected].map(paneOf));
  const write = $derived(session.can("ReadWrite"));

  const markChosen = async () => {
    for (const t of chosen) {
      const outcome =
        t.kind === "user" ? await chat.acknowledge(t.name) : await chat.markRoomReadNewest(t.name);
      if (!outcome.ok) toast(`${t.name}: ${outcome.error}`, "error");
    }
  };
  const closeChosen = async () => {
    for (const t of chosen) {
      const outcome = t.kind === "user" ? await chat.close(t.name) : await chat.leave(t.name);
      if (!outcome.ok) toast(`${t.name}: ${outcome.error}`, "error");
    }
    list.clear();
  };

  /** every conversation and room with something unread */
  const unreadChats = $derived([
    ...chat.conversations.rows
      .filter((c) => chat.unreadOf(c.username) > 0)
      .map((c): Target => ({ kind: "user", name: c.username })),
    ...chat.rooms.rows
      .filter((r) => chat.roomUnread(r.name).unread > 0)
      .map((r): Target => ({ kind: "room", name: r.name }))
  ]);
  const markAllRead = async () => {
    const targets = unreadChats;
    let failed = 0;
    for (const t of targets) {
      const outcome =
        t.kind === "user" ? await chat.acknowledge(t.name) : await chat.markRoomReadNewest(t.name);
      if (!outcome.ok) failed += 1;
    }
    if (failed) toast(`Could not mark ${plural(failed, "chat", "chats")} as read`, "error");
  };
</script>

<div class="flex min-h-0 flex-col gap-2 p-2">
  <SearchBox {q} {kind} {onjoin} {onnew} bind:input={search} />
  {#if write && unreadChats.length}
    <Button
      variant="ghost"
      size="sm"
      class="self-end text-xs text-fg-muted"
      onclick={() => void markAllRead()}>
      <CheckCheckIcon />Mark all read
    </Button>
  {/if}
  {#if chat.error}
    <Notice kind="error">
      {chat.error}
      {#snippet action()}
        <Button variant="ghost" size="sm" onclick={() => void chat.load()}>Retry</Button>
      {/snippet}
    </Notice>
  {/if}

  <div
    class="min-h-0 flex-1 overflow-y-auto rounded-control outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Rooms and conversations"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed}>
    {#each lines as line, i (line.key)}
      {#if line.kind === "header"}
        <div
          role="row"
          {...list.line(line, i)}
          class="flex items-center gap-1 px-1 pt-2 pb-1 text-xs font-medium text-fg-muted">
          <button
            type="button"
            class="flex items-center gap-1 hover:text-fg"
            aria-expanded={!folded.has(line.key)}
            onclick={() => (folded.has(line.key) ? folded.delete(line.key) : folded.add(line.key))}>
            {#if folded.has(line.key)}<ChevronRightIcon class="size-3.5" />{:else}<ChevronDownIcon
                class="size-3.5" />{/if}
            {line.title}
          </button>
        </div>
      {:else}
        {@const t = line.target}
        {@const room = t.kind === "room" ? chat.rooms.get(t.name) : undefined}
        {@const conversation = t.kind === "user" ? chat.conversations.get(t.name) : undefined}
        {@const last = conversation?.lastMessage}
        {@const preview = last
          ? { author: "", text: last.message, at: last.timestamp }
          : chat.previews.get(line.key)}
        {@const counts = t.kind === "room" ? chat.roomUnread(t.name) : null}
        {@const unread = t.kind === "user" ? chat.unreadOf(t.name) : 0}
        {@const notify = (room ?? conversation)?.notify}
        {@const summary = t.kind === "user" ? users.get(t.name) : undefined}
        {@const at = preview?.at ?? room?.lastMessageAt}
        <div
          role="row"
          {...list.line(line, i)}
          transition:disclose
          aria-current={current === line.key ? "page" : undefined}
          class={[
            "group/row flex items-center gap-2 rounded-control px-2 py-1.5 hover:bg-hover",
            current === line.key && "bg-hover",
            list.selected.has(line.key) && "bg-accent-soft/60"
          ]}
          oncontextmenu={(e) => {
            e.preventDefault();
            menuFor = line.key;
          }}>
          {#if t.kind === "user"}
            <Avatar name={t.name} pictureUrl={summary?.info?.pictureUrl} class="size-8 text-xs" />
          {:else}
            <span
              class="flex size-8 shrink-0 items-center justify-center rounded-full bg-surface-muted"
              ><HashIcon class="size-4 text-fg-muted" /></span>
          {/if}
          <span role="gridcell" class="flex min-w-0 flex-1 flex-col">
            <span class="flex items-center gap-1">
              <span
                class="truncate text-sm font-medium"
                style:color={t.kind === "user" ? nameColor(t.name) : undefined}>{t.name}</span>
              {#if notify === "none" || summary?.muted}<BellOffIcon
                  class="size-3 shrink-0 text-fg-subtle"
                  aria-label="muted" />{/if}
              {#if summary?.block}<Badge tone="danger">blocked</Badge>{/if}
              {#if at}<span class="ml-auto shrink-0 text-xs text-fg-subtle"
                  >{ago(at, clock.now)}</span
                >{/if}
            </span>
            <span class="flex items-center gap-1">
              <span class="truncate text-xs text-fg-muted">
                {#if preview}{@const action = actionOf(preview.text)}{preview.author &&
                  t.kind === "room"
                    ? `${preview.author}${action ? " " : ": "}`
                    : ""}{action ?? preview.text}{:else if t.kind === "room"}{plural(
                    room?.userCount ?? 0,
                    "user",
                    "users"
                  )}{/if}
              </span>
              {#if counts?.mentions}
                <Badge tone="accent" class="ml-auto" title="mentions">@{counts.mentions}</Badge>
              {:else if counts?.unread}
                <Badge class="ml-auto">{counts.unread}</Badge>
              {:else if unread}
                <Badge tone="danger" class="ml-auto">{unread}</Badge>
              {/if}
            </span>
          </span>
          <Menu.Root
            open={menuFor === line.key}
            onOpenChange={(o) => (menuFor = o ? line.key : null)}>
            <Menu.Trigger>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  class="opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
                  aria-label={`Menu for ${t.name}`}><EllipsisIcon /></Button>
              {/snippet}
            </Menu.Trigger>
            <Menu.Content align="end" class="min-w-52"><ChatMenuItems target={t} /></Menu.Content>
          </Menu.Root>
        </div>
      {/if}
    {/each}
  </div>

  {#if chosen.length > 1}
    <div
      class="flex flex-wrap items-center gap-1 rounded-control border bg-overlay p-1 text-sm"
      role="toolbar"
      aria-label="Selected chats">
      <span class="px-1">{chosen.length} selected</span>
      {#if write}
        <Button size="sm" variant="ghost" onclick={() => void markChosen()}>Mark read</Button>
        <Button size="sm" variant="ghost" onclick={() => void closeChosen()}>Close</Button>
      {/if}
      <Button size="sm" variant="ghost" onclick={() => list.clear()}>Clear</Button>
    </div>
  {/if}

  {#if write}
    <!-- side by side while both fit, one over the other at a large text size -->
    <div class="flex flex-wrap gap-1">
      <Button variant="outline" size="sm" class="flex-auto" onclick={() => onjoin()}
        ><UsersRoundIcon />Join a room</Button>
      <Button variant="outline" size="sm" class="flex-auto" onclick={() => onnew()}
        ><MessageSquarePlusIcon />New message</Button>
    </div>
  {/if}
</div>
