<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import HashIcon from "@lucide/svelte/icons/hash";
  import LockIcon from "@lucide/svelte/icons/lock";
  import MessagesSquareIcon from "@lucide/svelte/icons/messages-square";
  import PanelRightIcon from "@lucide/svelte/icons/panel-right";
  import ServerIcon from "@lucide/svelte/icons/server";
  import { Dialog, Tabs } from "bits-ui";
  import type { Snippet } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { chat, paneKey } from "#lib/chat/chat.svelte.js";
  import { columns } from "#lib/chat/columns.svelte.js";
  import type { Message } from "#lib/chat/messages/messages.js";
  import { chatName, isNotices } from "#lib/chat/notices.js";
  import { MessagePane, type Target } from "#lib/chat/pane.svelte.js";
  import { chatLayout as layout, SIDE_TABS, type SideTab } from "#lib/chat/prefs.svelte.js";
  import { RoomMembers } from "#lib/chat/rooms/room.svelte.js";
  import MessageList from "#lib/components/chat/messages/MessageList.svelte";
  import RoomActivity from "#lib/components/chat/rooms/RoomActivity.svelte";
  import RoomUsers from "#lib/components/chat/rooms/RoomUsers.svelte";
  import RoomWall from "#lib/components/chat/rooms/RoomWall.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import ResizeHandle from "#lib/components/ui/ResizeHandle.svelte";
  import Scrim from "#lib/components/ui/Scrim.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Thumb from "#lib/components/ui/Thumb.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Presence from "#lib/components/users/Presence.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { formatCount, plural } from "#lib/format.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import ChatMenuItems from "./ChatMenuItems.svelte";
  import Composer from "./Composer.svelte";

  type Props = {
    target: Target;
    /** a message to open the pane at, from a link */
    at?: number | null;
    /** shown before the name, such as the button that opens the chat list on a narrow screen */
    lead?: Snippet;
  };

  let { target, at = null, lead }: Props = $props();

  // svelte-ignore state_referenced_locally
  const pane = new MessagePane(target, () => app.state?.user.username ?? "", at);
  pane.follow();
  // svelte-ignore state_referenced_locally
  const members = target.kind === "room" ? new RoomMembers(target.name) : null;
  members?.follow();

  // the messages and a room's members each reload once their own list lets go
  keepCurrent(
    () => void pane.load(),
    () => (target.kind === "room" ? "rooms" : "conversations"),
    pane.messages.freeze
  );
  if (members)
    keepCurrent(
      () => void members.load(),
      () => "rooms",
      members.wallFreeze
    );
  void pane.load();
  void members?.load();

  // this pane is on screen: no notification for it while the tab is visible
  $effect(() => {
    const key = paneKey(target.kind, target.name);
    chat.openPane = key;
    return () => {
      if (chat.openPane === key) chat.openPane = null;
    };
  });
  const notices = $derived(isNotices(target));
  $effect(() => {
    if (target.kind === "user" && !notices) void users.load(target.name);
  });

  const write = $derived(session.can("ReadWrite"));
  const room = $derived(target.kind === "room" ? chat.rooms.get(target.name) : undefined);
  // a room the server refused after a login is listed but not joined
  const joined = $derived(target.kind === "room" ? !!room && !room.refused : true);
  const summary = $derived(target.kind === "user" ? users.get(target.name) : undefined);
  const disabled = $derived.by(() => {
    if (!write) return "Read-only accounts cannot send messages";
    if (!joined) return "Join the room to send messages";
    if (!app.isServerConnected) return "Connect to the Soulseek server to send messages";
    if (summary?.block?.scopes.includes("messages")) return "Messages from this user are blocked";
    return null;
  });
  /** the words marked in messages: our name and the watched words from the options */
  const watch = $derived.by(() => {
    const watched = app.option("chat.highlight_words");
    const us = app.state?.user.username;
    return [
      ...(us ? [us] : []),
      ...(Array.isArray(watched) ? watched.filter((w) => typeof w === "string") : [])
    ];
  });
  // the side panel sits beside the messages on a wide screen and in a sheet on a phone
  let sheet = $state(false);
  const sideShown = $derived(!!members && joined && !viewport.phone && layout.users);
  $effect(() => {
    columns.sideShown = sideShown;
    return () => (columns.sideShown = false);
  });

  let composer = $state<Composer | null>(null);
  const reply = (message: Message) => composer?.quote(message);

  const join = async () => {
    const outcome = await chat.join(target.name, false);
    if (!outcome.ok) toast(outcome.error, "error");
    else void members?.load();
  };
  // buddies come first in `@` completion
  void users.loadBuddies();
  // the people `@` can name, never ourselves and each once: the buddies among those here, the
  // others here, then the buddies who aren't
  const names = () => {
    const us = app.state?.user.username;
    const here = new Set(
      target.kind === "room"
        ? members!.users.rows.map((u) => u.username)
        : [target.name, ...chat.conversations.rows.map((c) => c.username)]
    );
    if (us) here.delete(us);
    const buddies = [...users.buddies]
      .filter((name) => name !== us)
      .toSorted((a, b) => a.localeCompare(b));
    return [
      ...buddies.filter((name) => here.has(name)),
      ...[...here].filter((name) => !users.buddies.has(name)),
      ...buddies.filter((name) => !here.has(name))
    ];
  };
</script>

{#snippet side(members: RoomMembers)}
  <!-- the values come only from SIDE_TABS, so the string bits-ui gives back is one of its ids -->
  <Tabs.Root bind:value={() => layout.side, (v) => (layout.side = v as SideTab)} class="contents">
    <Tabs.List aria-label="Side panel" class="relative flex gap-1 border-b p-1.5">
      <Thumb value={layout.side} class="rounded-control nav-current" />
      {#each SIDE_TABS as t (t.id)}
        <Tabs.Trigger
          value={t.id}
          class="relative z-1 flex h-control-sm flex-1 items-center justify-center rounded-control text-sm text-fg-muted transition-colors duration-(--motion-fast) hover:text-fg data-[state=active]:text-fg data-[state=inactive]:hover:bg-hover">
          {t.title}
          {#if t.id === "wall" && members.shownWall.length}
            <span class="ml-1 text-xs text-fg-muted tabular-nums"
              >{formatCount(members.shownWall.length)}</span>
          {/if}
        </Tabs.Trigger>
      {/each}
    </Tabs.List>
    <!-- only the open tab's panel draws its list -->
    {#each SIDE_TABS as t (t.id)}
      <Tabs.Content value={t.id} class="flex min-h-0 flex-1 outline-none">
        {#if layout.side === t.id}
          {#if t.id === "wall"}
            <RoomWall {members} />
          {:else if t.id === "activity"}
            <RoomActivity {members} />
          {:else}
            <RoomUsers {members} />
          {/if}
        {/if}
      </Tabs.Content>
    {/each}
  </Tabs.Root>
{/snippet}

<section
  class="flex min-h-0 min-w-0 flex-1 flex-col"
  aria-label={notices ? "Server notices" : `Chat with ${target.name}`}>
  <header class="flex min-h-12 items-center gap-2 border-b px-3 py-2">
    {@render lead?.()}
    {#if notices}
      <ServerIcon class="size-4 shrink-0 text-fg-muted" />
      <div class="name-count flex-1 gap-2">
        <h2 class="truncate font-medium">{chatName(target)}</h2>
        <span class="text-xs text-fg-muted"><span>Notices from the Soulseek server</span></span>
      </div>
    {:else if target.kind === "user"}
      <Presence presence={summary?.presence} worded={!!summary?.presence} />
      <div class="name-count flex-1 gap-2">
        <Username name={target.name} class="text-base" />
        <span class="text-xs text-fg-muted"><span>{summary?.presence ?? ""}</span></span>
      </div>
    {:else}
      {#if members?.room?.isPrivate}
        <LockIcon class="size-4 shrink-0 text-fg-muted" aria-label="Private room" />
      {:else}
        <HashIcon class="size-4 shrink-0 text-fg-muted" />
      {/if}
      <div class="name-count flex-1 gap-2">
        <h2 class="truncate font-medium">{target.name}</h2>
        <span class="text-xs text-fg-muted"
          ><span
            >{room?.refused
              ? "Not joined"
              : plural(members?.users.size ?? 0, "user", "users")}</span
          ></span>
      </div>
    {/if}
    <div class="ml-auto flex shrink-0 items-center gap-1">
      {#if target.kind === "room" && !joined && write}
        <Button size="sm" onclick={() => void join()}>Join</Button>
      {/if}
      {#if target.kind === "room" && joined}
        {#if !viewport.phone}
          <Tooltip text={layout.users ? "Hide side panel" : "Show side panel"}>
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label={layout.users ? "Hide side panel" : "Show side panel"}
                aria-pressed={layout.users}
                onclick={() => (layout.users = !layout.users)}><PanelRightIcon /></Button>
            {/snippet}
          </Tooltip>
        {:else}
          <Tooltip text="Show side panel">
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label="Show side panel"
                onclick={() => (sheet = true)}><PanelRightIcon /></Button>
            {/snippet}
          </Tooltip>
        {/if}
      {/if}
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label={target.kind === "room"
                ? "Room menu"
                : notices
                  ? "Server notices menu"
                  : "Conversation menu"}><EllipsisIcon /></Button>
          {/snippet}
        </Menu.Trigger>
        <Menu.Content align="end" class="min-w-56"><ChatMenuItems {target} /></Menu.Content>
      </Menu.Root>
    </div>
  </header>
  <div class="flex min-h-0 flex-1">
    <div class="flex min-h-0 min-w-0 flex-1 flex-col">
      {#if pane.error}
        <Notice kind="error">
          {pane.error}
          {#snippet action()}
            <Button variant="ghost" size="sm" onclick={() => void pane.load()}>Retry</Button>
          {/snippet}
        </Notice>
      {/if}
      {#if !pane.loaded}
        <div class="flex flex-1 flex-col justify-end gap-4 p-3" aria-hidden="true">
          {#each [0, 1, 2] as n (n)}
            <div class="flex gap-2.5">
              <Skeleton class="size-9 rounded-full" />
              <div class="flex flex-1 flex-col gap-1.5">
                <Skeleton class="h-3 w-32" /><Skeleton
                  class={n === 1 ? "h-3 w-2/3" : "h-3 w-1/2"} />
              </div>
            </div>
          {/each}
        </div>
      {:else if !pane.ordered.length && !pane.gaps.length && !pane.error}
        <EmptyState
          icon={notices ? ServerIcon : MessagesSquareIcon}
          class="flex-1 justify-center"
          title={notices ? "No notices from the server" : "No messages yet"} />
      {:else}
        <MessageList
          {pane}
          {watch}
          onreply={disabled || notices ? undefined : reply}
          onread={() => write && void pane.markRead()} />
      {/if}
      {#if target.kind === "room" && !joined && !write}
        <p class="border-t px-3 py-2 text-xs text-fg-muted">
          Not joined, showing the stored history
        </p>
      {:else if !notices}
        <Composer bind:this={composer} {pane} {disabled} {names} />
      {/if}
    </div>
    {#if members && sideShown}
      <!-- the side panel scrolls inside its own column, never past the pane -->
      {@const { width, min, max, initial } = columns.sideColumn}
      <div class="relative flex min-h-0 shrink-0 flex-col border-l" style:width={`${width}px`}>
        <ResizeHandle
          bind:value={() => width, (w) => (columns.side = w)}
          {min}
          {max}
          {initial}
          edge="left"
          label="Side panel width"
          onchange={(w) => (layout.sideWidth = w)}
          class="absolute inset-y-0 -left-1 z-10 w-2" />
        {@render side(members)}
      </div>
    {/if}
  </div>
</section>

{#if members && joined && viewport.phone}
  <Dialog.Root bind:open={sheet}>
    <Dialog.Portal>
      <Scrim />
      <Dialog.Content
        class="fixed inset-y-0 right-0 z-50 box-content flex w-80 max-w-[90vw] flex-col bg-surface safe-t safe-r safe-b shadow-raised [--sheet-from:100%] data-[state=closed]:animate-sheet-out data-[state=open]:animate-sheet-in">
        <Dialog.Title class="sr-only">{`#${target.name}`}</Dialog.Title>
        {@render side(members)}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}
