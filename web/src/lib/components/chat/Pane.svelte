<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import HashIcon from "@lucide/svelte/icons/hash";
  import PanelRightIcon from "@lucide/svelte/icons/panel-right";
  import type { Snippet } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { chat, paneKey } from "#lib/chat/chat.svelte.js";
  import { MessagePane, type Target } from "#lib/chat/pane.svelte.js";
  import { chatLayout as layout } from "#lib/chat/prefs.svelte.js";
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
  import { toast } from "#lib/toast.svelte.js";
  import { users } from "#lib/users.svelte.js";

  import ChatMenuItems from "./ChatMenuItems.svelte";
  import Composer from "./Composer.svelte";
  import MessageList from "./MessageList.svelte";
  import RoomUsers from "./RoomUsers.svelte";

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
  const tickers = $derived(members?.room?.tickers ?? []);

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
  <header class="flex items-center gap-2 border-b px-3 py-2">
    {@render lead?.()}
    {#if target.kind === "user"}
      <Presence presence={summary?.presence} />
      <Username name={target.name} class="text-base" />
      <span class="text-xs text-fg-muted">{summary?.presence ?? ""}</span>
    {:else}
      <HashIcon class="size-4 text-fg-muted" />
      <h2 class="truncate font-semibold">{target.name}</h2>
      <span class="text-xs text-fg-muted"
        >{plural(members?.users.size ?? 0, "1 user", `${members?.users.size ?? 0} users`)}</span>
    {/if}
    <div class="ml-auto flex items-center gap-1">
      {#if target.kind === "room" && !joined && write}
        <Button size="sm" onclick={() => void join()}>Join</Button>
      {/if}
      {#if target.kind === "room" && joined}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={layout.users ? "Hide the users" : "Show the users"}
          aria-pressed={layout.users}
          onclick={() => (layout.users = !layout.users)}><PanelRightIcon /></Button>
      {/if}
      <Menu.Root>
        <Menu.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-sm" aria-label="Conversation menu"
              ><EllipsisIcon /></Button>
          {/snippet}
        </Menu.Trigger>
        <Menu.Content align="end" class="min-w-52"><ChatMenuItems {target} /></Menu.Content>
      </Menu.Root>
    </div>
  </header>
  {#if tickers.length}
    <p
      class="truncate border-b px-3 py-1 text-xs text-fg-muted"
      title={tickers.map((t) => `${t.username}: ${t.ticker}`).join("\n")}>
      {#each tickers as t (t.username)}<span class="mr-4"
          ><span class="font-medium">{t.username}</span> {t.ticker}</span
        >{/each}
    </p>
  {/if}
  <div class="flex min-h-0 flex-1">
    <div class="flex min-h-0 min-w-0 flex-1 flex-col">
      {#if pane.error}<Notice kind="error">{pane.error}</Notice>{/if}
      {#if !pane.loaded}
        <div class="flex flex-col gap-2 p-3">
          <Skeleton class="h-12" /><Skeleton class="h-12" />
        </div>
      {:else}
        <MessageList {pane} {at} onread={() => write && void pane.markRead()} />
      {/if}
      <Composer {pane} {disabled} {names} />
    </div>
    {#if members && joined && layout.users}
      <div class="hidden w-60 shrink-0 md:flex">
        <RoomUsers {members} />
      </div>
    {/if}
  </div>
</section>
