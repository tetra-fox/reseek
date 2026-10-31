<script lang="ts">
  import { goto } from "$app/navigation";
  import MessageSquarePlusIcon from "@lucide/svelte/icons/message-square-plus";
  import SearchIcon from "@lucide/svelte/icons/search";
  import UsersRoundIcon from "@lucide/svelte/icons/users-round";
  import { Combobox } from "bits-ui";
  import type { Snippet } from "svelte";

  import { chatName, SERVER } from "#lib/chat/notices.js";
  import { foundHref, resultsHref } from "#lib/chat/route.js";
  import {
    dropdown,
    type HardResult,
    hitTarget,
    MAX_TEXT,
    type MessageHit,
    type SearchKind
  } from "#lib/chat/search/match.js";
  import { ChatSearch } from "#lib/chat/search/search.svelte.js";
  import Input from "#lib/components/ui/Input.svelte";
  import { ago, dateTime } from "#lib/format.js";
  import { chatHref, roomHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { snippetParts } from "#lib/text.js";
  import { clock } from "#lib/ui/clock.svelte.js";

  import Marked from "./Marked.svelte";
  import RoomFound from "./RoomFound.svelte";
  import UserFound from "./UserFound.svelte";

  type Props = {
    /** the search the address holds, which the box shows */
    q: string;
    /** the messages the address searches */
    kind: SearchKind;
    onjoin: (name: string) => void;
    onnew: (username: string) => void;
    /** the box, for Ctrl/Cmd+K */
    input?: HTMLInputElement | null;
  };

  let { q, kind, onjoin, onnew, input = $bindable(null) }: Props = $props();

  /** how many messages are read for each text, the best of them are listed */
  const HITS_READ = 20;

  const ALL = "all";
  const NEW = "new";
  const JOIN = "join";

  let typed = $derived(q);
  let open = $state(false);
  /** what the dropdown reports as chosen, emptied at once so the same entry can be chosen again */
  let chosen = $state("");
  /** the entry the keys or the pointer are on */
  let lit = $state<string | null>(null);

  // no name has spaces around it, so what is around the text is no part of the search
  const text = $derived(typed.trim());
  const box = new ChatSearch(
    () => text,
    () => "all",
    HITS_READ
  );
  const listed = $derived(dropdown(box.found, text));
  const noName = $derived(!box.found.rooms.length && !box.found.users.length);
  const write = $derived(session.can("ReadWrite"));

  const roomValue = (name: string) => `room:${name}`;
  const userValue = (name: string) => `user:${name}`;
  const hitValue = (hit: MessageHit) => `hit:${hit.kind}:${hit.localId}`;
  const hardValue = (hard: HardResult) =>
    hard.kind === "room" ? roomValue(hard.room.name) : userValue(hard.user.name);
  /** the entry Enter takes unless the keys moved on: the hard result, else every result */
  const first = $derived(listed.hard ? hardValue(listed.hard) : ALL);

  $effect(() => {
    void text;
    const timer = setTimeout(() => {
      if (!box.current) void box.read();
    }, 200);
    return () => clearTimeout(timer);
  });

  /** Opens a chat the box named: the jump is done, so the text goes with it */
  const openChat = (href: string) => {
    typed = "";
    void goto(href);
  };

  const choose = (value: string) => {
    chosen = "";
    open = false;
    lit = null;
    const hit = listed.hits.find((h) => hitValue(h) === value);
    if (hit) void goto(foundHref(hitTarget(hit), text, "all", hit.localId));
    else if (value === ALL) void goto(resultsHref(text, kind));
    else if (value === NEW) onnew(text);
    else if (value === JOIN) onjoin(text);
    else if (value.startsWith("room:")) openChat(roomHref(value.slice(5)));
    else if (value.startsWith("user:")) openChat(chatHref(value.slice(5)));
  };

  /**
   * Enter takes the entry the keys moved to. Otherwise it takes the first, once the daemon has
   * answered for the text, so a name typed quickly and known only there still opens its chat
   */
  const enter = async () => {
    if (lit !== null && lit !== first) return choose(lit);
    await box.settled();
    choose(first);
  };

  const keydown = (e: KeyboardEvent) => {
    if (e.key !== "Enter" || e.isComposing || !text) return;
    e.preventDefault();
    void enter();
  };
</script>

{#snippet item(value: string, body: Snippet)}
  <Combobox.Item
    {value}
    label={text}
    onHighlight={() => (lit = value)}
    onUnhighlight={() => {
      if (lit === value) lit = null;
    }}
    class="flex items-center gap-2 rounded-badge px-2 py-1 text-sm data-highlighted:bg-hover">
    {@render body()}
  </Combobox.Item>
{/snippet}

{#snippet everything()}
  {#snippet body()}
    <SearchIcon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
    <span class="min-w-0 truncate">All results for "{text}"</span>
  {/snippet}
  {@render item(ALL, body)}
{/snippet}

{#snippet heading(title: string)}
  <Combobox.GroupHeading class="px-2 pt-2 pb-0.5 text-xs font-medium text-fg-muted">
    {title}
  </Combobox.GroupHeading>
{/snippet}

<Combobox.Root
  type="single"
  bind:open={() => open && !!text, (on) => (open = on)}
  bind:value={chosen}
  onValueChange={choose}>
  <Combobox.Input oninput={() => (open = true)} onclick={() => (open = true)} onkeydown={keydown}>
    {#snippet child({ props })}
      <Input
        {...props}
        bind:ref={input}
        bind:value={typed}
        icon={SearchIcon}
        type="search"
        autocomplete="off"
        maxlength={MAX_TEXT}
        placeholder="Rooms, users and messages"
        aria-label="Search chats" />
    {/snippet}
  </Combobox.Input>
  <Combobox.Portal>
    <Combobox.Content
      sideOffset={4}
      class="z-50 flex max-h-(--bits-combobox-content-available-height) w-[max(var(--bits-combobox-anchor-width),20rem)] max-w-[calc(100vw-1rem)] origin-(--bits-combobox-content-transform-origin) flex-col rounded-control border bg-overlay p-1 text-fg shadow-raised data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
      <Combobox.Viewport class="min-h-0">
        <!-- a new first entry takes the highlight: entries made anew move it to the first -->
        {#if listed.hard}
          {@const hard = listed.hard}
          {#key first}
            {#snippet body()}
              {#if hard.kind === "room"}
                <RoomFound room={hard.room} {text} />
              {:else}
                <UserFound user={hard.user} {text} />
              {/if}
            {/snippet}
            {@render item(first, body)}
          {/key}
        {:else}
          {@render everything()}
        {/if}
        {#if listed.rooms.length}
          <Combobox.Group>
            {@render heading("Rooms")}
            {#each listed.rooms as room (room.name)}
              {#snippet body()}<RoomFound {room} {text} />{/snippet}
              {@render item(roomValue(room.name), body)}
            {/each}
          </Combobox.Group>
        {/if}
        {#if listed.users.length}
          <Combobox.Group>
            {@render heading("Users")}
            {#each listed.users as user (user.name)}
              {#snippet body()}<UserFound {user} {text} />{/snippet}
              {@render item(userValue(user.name), body)}
            {/each}
          </Combobox.Group>
        {/if}
        {#if listed.hits.length}
          <Combobox.Group>
            {@render heading("Messages")}
            {#each listed.hits as hit (hitValue(hit))}
              {@const chat = hitTarget(hit)}
              {#snippet body()}
                <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span class="flex items-baseline gap-2 text-xs text-fg-muted">
                    <span class="min-w-0 truncate font-medium">
                      {chat.kind === "room" ? `#${chat.name}, ${hit.username}` : chatName(chat)}
                    </span>
                    <time
                      class="ml-auto shrink-0"
                      datetime={hit.timestamp}
                      title={dateTime(hit.timestamp)}>{ago(hit.timestamp, clock.now)}</time>
                  </span>
                  <span class="line-clamp-2 wrap-anywhere"
                    ><Marked parts={snippetParts(hit.snippet)} /></span>
                </span>
              {/snippet}
              {@render item(hitValue(hit), body)}
            {/each}
          </Combobox.Group>
        {/if}
        {#if noName && !listed.hits.length && box.current}
          <p class="px-2 py-1 text-sm text-fg-muted">No rooms, users or messages match.</p>
        {/if}
        {#if noName && write}
          {#snippet talk()}
            <MessageSquarePlusIcon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
            <span class="min-w-0 truncate"
              >Start a conversation with <span class="font-medium">{text}</span></span>
          {/snippet}
          {#snippet enterRoom()}
            <UsersRoundIcon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
            <span class="min-w-0 truncate">Join room <span class="font-medium">{text}</span></span>
          {/snippet}
          {#if text !== SERVER}{@render item(NEW, talk)}{/if}
          {@render item(JOIN, enterRoom)}
        {/if}
        {#if listed.hard}{@render everything()}{/if}
      </Combobox.Viewport>
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>
