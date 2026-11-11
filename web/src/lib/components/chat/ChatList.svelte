<script lang="ts">
  import { goto } from "$app/navigation";
  import BellOffIcon from "@lucide/svelte/icons/bell-off";
  import CheckCheckIcon from "@lucide/svelte/icons/check-check";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import GripVerticalIcon from "@lucide/svelte/icons/grip-vertical";
  import HashIcon from "@lucide/svelte/icons/hash";
  import MessageSquarePlusIcon from "@lucide/svelte/icons/message-square-plus";
  import RadioTowerIcon from "@lucide/svelte/icons/radio-tower";
  import ServerIcon from "@lucide/svelte/icons/server";
  import UsersRoundIcon from "@lucide/svelte/icons/users-round";
  import { tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import { chat, paneKey, paneOf } from "#lib/chat/chat.svelte.js";
  import { actionOf } from "#lib/chat/messages/rich.js";
  import { chatName, isNotices, NOTICES } from "#lib/chat/notices.js";
  import type { Target } from "#lib/chat/pane.svelte.js";
  import { pins, samePin } from "#lib/chat/pins.svelte.js";
  import { pinnedChats, sortedConversations, sortedRooms } from "#lib/chat/rooms/order.js";
  import type { SearchKind } from "#lib/chat/search/match.js";
  import SearchBox from "#lib/components/chat/search/SearchBox.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Avatar from "#lib/components/users/Avatar.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, badgeCount, formatCount, plural } from "#lib/format.js";
  import { freezesWhileUsed, heldOrder, interaction } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { RowDrag } from "#lib/lists/rowdrag.svelte.js";
  import { chatHref, publicFeedHref, roomHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { inControl, moveKeys, moveStep } from "#lib/ui/keys.js";
  import { disclose } from "#lib/ui/motion.js";
  import { nameColor } from "#lib/users/colors.js";
  import { pictureOf } from "#lib/users/profile.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import ChatMenuItems from "./ChatMenuItems.svelte";

  type Props = {
    /** the pane on screen, by pane key */
    current: string | null;
    /** the feed of every public room is on screen */
    feed?: boolean;
    /** the search the address holds, which the search box shows */
    q?: string;
    /** the messages the address searches, which a new search keeps */
    kind?: SearchKind;
    onjoin: (name?: string) => void;
    onnew: (username?: string) => void;
  };

  let { current, feed = false, q = "", kind = "all", onjoin, onnew }: Props = $props();

  type Line =
    | { kind: "header"; key: string; title: string; ids: string[] }
    | { kind: "row"; key: string; target: Target; ids: string[]; pinned: boolean };

  const PINNED = "h:pinned";

  /**
   * where a row's content starts, the same in a section's heading. Past the row's icon, the left
   * padding holds a pin's grip; on a touch screen the grip takes its own place in the row instead
   */
  const rowStart = "pl-4 pointer-coarse:pl-2";
  /** a count at a row's end, which keeps to the height of its line */
  const count = "-my-px ml-auto py-0 row-meta";

  const folded = new SvelteSet<string>();
  /** the row whose menu is open, from its button or a right click */
  let menuFor = $state<string | null>(null);

  const pinned = $derived(pinnedChats());
  const rooms = $derived(sortedRooms());
  const conversations = $derived(sortedConversations());

  // the list holds still while the pointer is on it, a text is selected in it or a row's menu
  // is open, so a row never moves away from a click
  const hold = heldOrder<Line>((l) => l.key);
  $effect(() => (menuFor ? interaction.hold() : undefined));
  const lines = $derived.by(() => {
    // an empty section shows no heading
    const section = (key: string, title: string, targets: Target[]): Line[] => {
      if (!targets.length) return [];
      const ids = targets.map((t) => paneKey(t.kind, t.name));
      return [
        { kind: "header", key, title, ids },
        ...(folded.has(key)
          ? []
          : targets.map((target): Line => ({
              kind: "row",
              key: paneKey(target.kind, target.name),
              target,
              ids,
              pinned: key === PINNED
            })))
      ];
    };
    return hold([
      ...section(PINNED, "Pinned", pinned),
      ...section(
        "h:rooms",
        "Rooms",
        rooms.map((r) => ({ kind: "room", name: r.name }))
      ),
      ...section(
        "h:conversations",
        "Conversations",
        conversations.map((c) => ({ kind: "user", name: c.username }))
      ),
      ...section("h:notices", "Notices", chat.latestNotice ? [NOTICES] : [])
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
    onchord: (e, l) => {
      const step = moveStep(e);
      if (!step || !write || l.kind !== "row" || !l.pinned) return false;
      nudge(l.target, step);
      return true;
    },
    openOnClick: true
  });

  /** what the last move of a pin came to, for a screen reader */
  let placed = $state("");
  /** Puts a pin in front of `before`, or last without one */
  const place = (pin: Target, before: Target | null) => {
    pins.move(pin, before);
    const shown = pinnedChats();
    const at = shown.findIndex((p) => samePin(p, pin));
    placed = `${chatName(pin)} moved to place ${at + 1} of ${shown.length} in Pinned`;
  };
  /** Moves a pin one place up or down, the row staying in view */
  const nudge = (pin: Target, step: -1 | 1) => {
    const shown = pinnedChats();
    const to = shown.findIndex((p) => samePin(p, pin)) + step;
    if (to < 0 || to >= shown.length) {
      placed = `${chatName(pin)} is already ${step < 0 ? "first" : "last"} in Pinned`;
      return;
    }
    place(pin, shown[step < 0 ? to : to + 1] ?? null);
    void tick().then(() =>
      document.getElementById(list.activeId ?? "")?.scrollIntoView({ block: "nearest" })
    );
  };

  let box = $state<HTMLElement | null>(null);
  const drag = new RowDrag(
    () => box,
    (key, before) => {
      place(paneOf(key), before ? paneOf(before) : null);
      // the row moved in the page, which took focus from a control in it
      list.focus();
    }
  );

  // every chat selected, those a folded section hides too
  const chosen = $derived([...list.selected].map(paneOf));
  // the server's notices are never unread, and they cannot be closed
  const chosenChats = $derived(chosen.filter((t) => !isNotices(t)));
  const chosenPinned = $derived(chosenChats.every((t) => pins.has(t)));
  const write = $derived(session.can("ReadWrite"));

  const markChosen = async () => {
    for (const t of chosenChats) {
      const outcome =
        t.kind === "user" ? await chat.acknowledge(t.name) : await chat.markRoomReadNewest(t.name);
      if (!outcome.ok) toast(`${t.name}: ${outcome.error}`, "error");
    }
  };
  const closeChosen = async () => {
    for (const t of chosenChats) {
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
  <SearchBox {q} {kind} {onjoin} {onnew} />
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

  <p class="sr-only" role="status">{placed}</p>

  <div
    bind:this={box}
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
        <div role="row" {...list.line(line, i)} class="rounded-control pt-1.5 pb-0.5">
          <button
            type="button"
            class={[
              "flex h-control-sm w-full items-center gap-1 rounded-control text-xs font-medium text-fg-muted hover:bg-hover hover:text-fg",
              rowStart
            ]}
            aria-expanded={!folded.has(line.key)}
            onclick={() => (folded.has(line.key) ? folded.delete(line.key) : folded.add(line.key))}>
            {#if folded.has(line.key)}<ChevronRightIcon class="size-3.5" />{:else}<ChevronDownIcon
                class="size-3.5" />{/if}
            {line.title}
          </button>
          {#if write && line.key === PINNED}<span class="sr-only">Move a pin with {moveKeys}.</span
            >{/if}
        </div>
      {:else}
        {@const t = line.target}
        {@const notices = isNotices(t)}
        {@const person = t.kind === "user" && !notices}
        {@const room = t.kind === "room" ? chat.rooms.get(t.name) : undefined}
        {@const conversation = person ? chat.conversations.get(t.name) : undefined}
        {@const last = notices ? chat.latestNotice : conversation?.lastMessage}
        {@const preview = last
          ? {
              author: last.direction === "Out" ? (app.state?.user.username ?? "") : t.name,
              text: last.message,
              at: last.timestamp
            }
          : room?.lastMessage
            ? {
                author: room.lastMessage.username,
                text: room.lastMessage.message,
                at: room.lastMessage.timestamp
              }
            : undefined}
        {@const action = preview ? actionOf(preview.text) : null}
        {@const author = t.kind === "room" || (person && action) ? preview?.author : ""}
        {@const counts = t.kind === "room" ? chat.roomUnread(t.name) : null}
        {@const unread = person ? chat.unreadOf(t.name) : 0}
        {@const notify = (room ?? conversation)?.notify}
        {@const summary = person ? users.get(t.name) : undefined}
        {@const at = preview?.at ?? room?.lastMessageAt}
        {@const movable = write && line.pinned}
        <div
          role="row"
          {...list.line(line, i)}
          transition:disclose
          aria-current={current === line.key ? "page" : undefined}
          aria-keyshortcuts={movable ? "Alt+Shift+ArrowUp Alt+Shift+ArrowDown" : undefined}
          style:translate={drag.shift(line.key)}
          class={[
            "relative flex items-center gap-2 pick-row py-1.5 pr-3",
            rowStart,
            movable && "select-none",
            drag.key === line.key && "z-10 cursor-grabbing bg-surface bg-clip-border shadow-raised",
            drag.key !== null &&
              drag.key !== line.key &&
              "transition-[translate] duration-(--motion-fast) ease-(--ease-out-soft)"
          ]}
          oncontextmenu={(e) => {
            e.preventDefault();
            menuFor = line.key;
          }}
          onpointerdown={(e) => {
            if (movable && e.pointerType === "mouse" && !inControl(e)) {
              drag.press(e, line.key, line.ids);
            }
          }}
          ondragstart={(e) => {
            if (movable) e.preventDefault();
          }}>
          {#if movable}
            <!-- the press is taken in the capture phase, so the tooltip's own pointerdown still
                 closes it -->
            <Tooltip text={`Drag to reorder, or press ${moveKeys}`}>
              {#snippet child({ props })}
                <button
                  {...props}
                  type="button"
                  tabindex="-1"
                  aria-hidden="true"
                  class="absolute inset-y-0 left-0 flex w-4 cursor-grab touch-none items-center justify-center text-fg-subtle opacity-0 hover:text-fg-muted active:cursor-grabbing pointer-coarse:static pointer-coarse:-mx-1 pointer-coarse:w-7 pointer-coarse:shrink-0 pointer-coarse:self-stretch pointer-coarse:opacity-100 row-active:opacity-100"
                  onpointerdowncapture={(e) => drag.press(e, line.key, line.ids)}
                  ><GripVerticalIcon class="size-3.5 shrink-0" /></button>
              {/snippet}
            </Tooltip>
          {/if}
          {#if person}
            <Avatar name={t.name} pictureUrl={pictureOf(t.name)} class="size-8 text-xs" />
          {:else}
            {@const Icon = notices ? ServerIcon : HashIcon}
            <span
              class="flex size-8 shrink-0 items-center justify-center rounded-full bg-surface-muted"
              ><Icon class="size-4 text-fg-muted" /></span>
          {/if}
          <!-- two lines' height on every row, so a row is as tall with a badge or without a preview,
               and a row with one line has it in the middle -->
          <span role="gridcell" class="flex h-9 min-w-0 flex-1 flex-col justify-center row-text">
            <span class="name-count gap-2">
              <span class="flex min-w-0 items-center gap-1">
                {#if t.kind === "room"}
                  <Tooltip text={t.name} clipped>
                    {#snippet child({ props })}
                      <span {...props} class="flex min-w-0 text-sm font-medium"
                        ><MiddleTruncate text={t.name} /></span>
                    {/snippet}
                  </Tooltip>
                {:else}
                  <span
                    class="truncate text-sm font-medium"
                    dir="auto"
                    style:color={person ? nameColor(t.name) : undefined}>{chatName(t)}</span>
                {/if}
                {#if notify === "none" || summary?.muted}<BellOffIcon
                    class="size-3 shrink-0 text-fg-subtle"
                    aria-label="Muted" />{/if}
                {#if summary?.block}<Badge tone="danger" class="-my-px py-0">blocked</Badge>{/if}
              </span>
              <span class="row-meta text-right text-xs text-fg-muted"
                ><span>{at ? ago(at, clock.now) : ""}</span></span>
            </span>
            <span class="flex items-center gap-2">
              <!-- a room's preview names who said it, a conversation's only an action, which needs
                   its subject. The name takes at most half, so some of the text always shows -->
              <span class="flex min-w-0 text-xs text-fg-muted">
                {#if preview}
                  {#if author}
                    <span class="max-w-1/2 shrink-0 truncate" dir="auto">{author}</span>
                    <span class="shrink-0 whitespace-pre">{action ? " " : ": "}</span>
                  {/if}
                  <span class="min-w-0 truncate" dir="auto">{action ?? preview.text}</span>
                {:else if t.kind === "room"}
                  <span class="truncate">{plural(room?.userCount ?? 0, "user", "users")}</span>
                {/if}
              </span>
              {#if counts?.mentions}
                <Tooltip text="Mentions" dense spoken>
                  {#snippet child({ props })}
                    <Badge {...props} tone="accent" class={count}
                      >@{badgeCount(counts.mentions)}</Badge>
                  {/snippet}
                </Tooltip>
              {:else if counts?.unread}
                <Badge class={count}>{badgeCount(counts.unread)}</Badge>
              {:else if unread}
                <Badge tone="danger" class={count}>{badgeCount(unread)}</Badge>
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
                  class="row-menu"
                  aria-label={`Menu for ${chatName(t)}`}><EllipsisIcon /></Button>
              {/snippet}
            </Menu.Trigger>
            <Menu.Content align="end" class="min-w-52"><ChatMenuItems target={t} /></Menu.Content>
          </Menu.Root>
        </div>
      {/if}
    {:else}
      <p class="px-4 py-2 text-sm text-fg-muted">No rooms or conversations yet</p>
    {/each}
  </div>

  {#if chosen.length > 1}
    <div
      class="flex flex-wrap items-center gap-1 rounded-control border bg-overlay p-1 text-sm"
      role="toolbar"
      aria-label="Selected chats">
      <span class="px-1">{formatCount(chosen.length)} selected</span>
      {#if write}
        <Button size="sm" variant="ghost" onclick={() => void markChosen()}>Mark read</Button>
        <Button size="sm" variant="ghost" onclick={() => pins.set(chosenChats, !chosenPinned)}
          >{chosenPinned ? "Unpin" : "Pin"}</Button>
        <Button size="sm" variant="ghost" onclick={() => void closeChosen()}>Close</Button>
      {/if}
      <Button size="sm" variant="ghost" onclick={() => list.clear()}>Clear</Button>
    </div>
  {/if}

  <!-- side by side while they fit, one over the other at a large text size -->
  <div class="flex flex-wrap gap-1">
    {#if write}
      <Button variant="outline" size="sm" class="flex-auto" onclick={() => onjoin()}
        ><UsersRoundIcon />Join room</Button>
      <Button variant="outline" size="sm" class="flex-auto" onclick={() => onnew()}
        ><MessageSquarePlusIcon />New message</Button>
    {/if}
    <Tooltip text="Live messages from every public room">
      {#snippet child({ props })}
        <Button
          {...props}
          variant="ghost"
          size="sm"
          class="flex-auto text-fg-muted aria-[current=page]:nav-current"
          href={publicFeedHref()}
          aria-current={feed ? "page" : undefined}><RadioTowerIcon />All public rooms</Button>
      {/snippet}
    </Tooltip>
  </div>
</div>
