<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import HashIcon from "@lucide/svelte/icons/hash";
  import LockIcon from "@lucide/svelte/icons/lock";
  import PanelRightIcon from "@lucide/svelte/icons/panel-right";
  import type { Snippet } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { chat, paneKey } from "#lib/chat/chat.svelte.js";
  import type { Message } from "#lib/chat/messages.js";
  import { MessagePane, type Target } from "#lib/chat/pane.svelte.js";
  import { chatLayout as layout, showsPresence } from "#lib/chat/prefs.svelte.js";
  import { fold } from "#lib/chat/presence.js";
  import { RoomMembers } from "#lib/chat/room.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Presence from "#lib/components/users/Presence.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { plural } from "#lib/format.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { users } from "#lib/users.svelte.js";

  import ChatMenuItems from "./ChatMenuItems.svelte";
  import Composer from "./Composer.svelte";
  import MessageList from "./MessageList.svelte";
  import RoomUsers from "./RoomUsers.svelte";
  import RoomWall from "./RoomWall.svelte";

  type Props = {
    target: Target;
    /** a message to open the pane at, from a link */
    at?: number | null;
    /** shown before the name, such as the button that opens the chat list on a narrow screen */
    lead?: Snippet;
  };

  let { target, at = null, lead }: Props = $props();

  // svelte-ignore state_referenced_locally
  const pane = new MessagePane(target, () => app.state?.user.username ?? "");
  pane.follow();
  // svelte-ignore state_referenced_locally
  const members = target.kind === "room" ? new RoomMembers(target.name) : null;
  members?.follow();

  keepCurrent(
    () => {
      void pane.load();
      void members?.load();
    },
    () => (target.kind === "room" ? "rooms" : "conversations")
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
  $effect(() => {
    if (target.kind === "user") void users.load(target.name);
  });

  const write = $derived(session.can("ReadWrite"));
  const joined = $derived(target.kind === "room" ? !!chat.rooms.get(target.name) : true);
  const summary = $derived(target.kind === "user" ? users.get(target.name) : undefined);
  const disabled = $derived.by(() => {
    if (!write) return "Read-only accounts can't send messages";
    if (target.kind === "user" && target.name === "server")
      return "The server's notices can't be answered";
    if (!joined) return "Join the room to send messages";
    if (!app.isServerConnected) return "Not connected to Soulseek";
    if (summary?.block?.scopes.includes("messages")) return "You blocked them from messaging";
    return null;
  });
  /** the words marked in messages: our name and the watched words from the options */
  const watch = $derived.by(() => {
    const words = fields(app.options).chat;
    const watched = fields(words).highlight_words;
    const us = app.state?.user.username;
    return [
      ...(us ? [us] : []),
      ...(Array.isArray(watched) ? watched.filter((w) => typeof w === "string") : [])
    ];
  });
  const bursts = $derived(members && showsPresence(target.name) ? fold(members.presence) : []);

  let composer = $state<Composer | null>(null);
  const reply = (message: Message) => composer?.quote(message);

  const join = async () => {
    const outcome = await chat.join(target.name, false);
    if (!outcome.ok) toast(outcome.error, "error");
    else void members?.load();
  };
  const names = () =>
    target.kind === "room"
      ? members!.users.rows.map((u) => u.username)
      : [target.name, ...chat.conversations.rows.map((c) => c.username)];
</script>

<section class="flex min-h-0 min-w-0 flex-1 flex-col" aria-label={`Chat with ${target.name}`}>
  <header class="flex min-h-12 items-center gap-2 border-b px-3 py-2">
    {@render lead?.()}
    {#if target.kind === "user"}
      <Presence presence={summary?.presence} />
      <Username name={target.name} class="text-base" />
      <span class="text-xs text-fg-muted">{summary?.presence ?? ""}</span>
    {:else}
      {#if members?.room?.isPrivate}
        <LockIcon class="size-4 text-fg-muted" aria-label="private room" />
      {:else}
        <HashIcon class="size-4 text-fg-muted" />
      {/if}
      <h2 class="truncate font-semibold">{target.name}</h2>
      <span class="text-xs whitespace-nowrap text-fg-muted"
        >{plural(members?.users.size ?? 0, "user", "users")}</span>
    {/if}
    <div class="ml-auto flex items-center gap-1">
      {#if target.kind === "room" && !joined && write}
        <Button size="sm" onclick={() => void join()}>Join</Button>
      {/if}
      {#if target.kind === "room" && joined}
        <Button
          variant="ghost"
          size="icon-sm"
          class="max-md:hidden"
          aria-label={layout.users ? "Hide the side panel" : "Show the users and the wall"}
          aria-pressed={layout.users}
          onclick={() => (layout.users = !layout.users)}><PanelRightIcon /></Button>
      {/if}
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label={target.kind === "room" ? "Room menu" : "Conversation menu"}
              ><EllipsisIcon /></Button>
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
      {:else}
        <MessageList
          {pane}
          {at}
          {bursts}
          {watch}
          onreply={disabled ? undefined : reply}
          onread={() => write && void pane.markRead()} />
      {/if}
      {#if target.kind === "room" && !joined && !write}
        <p class="border-t px-3 py-2 text-xs text-fg-muted">
          This is the stored history of a room you are not in.
        </p>
      {:else}
        <Composer bind:this={composer} {pane} {disabled} {names} />
      {/if}
    </div>
    {#if members && joined && layout.users}
      <!-- the side panel scrolls inside its own column, never past the pane -->
      <div class="hidden min-h-0 w-64 shrink-0 flex-col border-l md:flex">
        <div role="tablist" aria-label="Side panel" class="flex gap-1 border-b p-1.5">
          {#each [{ id: "users", title: "Users" }, { id: "wall", title: "Wall" }] as t (t.id)}
            <button
              type="button"
              role="tab"
              aria-selected={layout.side === t.id}
              class="flex h-control-sm flex-1 items-center justify-center rounded-control text-sm text-fg-muted hover:bg-hover hover:text-fg aria-selected:nav-current"
              onclick={() => (layout.side = t.id as "users" | "wall")}>
              {t.title}
              {#if t.id === "wall" && members.wall.length}
                <span class="ml-1 text-xs text-fg-subtle tabular-nums">{members.wall.length}</span>
              {/if}
            </button>
          {/each}
        </div>
        <div role="tabpanel" class="flex min-h-0 flex-1">
          {#if layout.side === "wall"}
            <RoomWall {members} />
          {:else}
            <RoomUsers {members} />
          {/if}
        </div>
      </div>
    {/if}
  </div>
</section>
