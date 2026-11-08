<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import MessageSquarePlusIcon from "@lucide/svelte/icons/message-square-plus";
  import SearchIcon from "@lucide/svelte/icons/search";
  import UsersRoundIcon from "@lucide/svelte/icons/users-round";

  import { chatName, SERVER } from "#lib/chat/notices.js";
  import { foundHref, resultsHref } from "#lib/chat/route.js";
  import {
    dropdown,
    type HardResult,
    hitTarget,
    MAX_TEXT,
    type MessageHit
  } from "#lib/chat/search/match.js";
  import { ChatSearch } from "#lib/chat/search/search.svelte.js";
  import Marked from "#lib/components/ui/Marked.svelte";
  import * as Suggest from "#lib/components/ui/suggest/suggest.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import UserEntry from "#lib/components/users/UserEntry.svelte";
  import { ago, dateTime } from "#lib/format.js";
  import { chatHref, roomHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { snippetParts } from "#lib/text.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { TYPING_PAUSE } from "#lib/ui/keys.js";

  import RoomFound from "./RoomFound.svelte";

  type Props = {
    /** the search the address holds, which the box shows */
    q: string;
    onjoin: (name: string) => void;
    onnew: (username: string) => void;
  };

  let { q, onjoin, onnew }: Props = $props();

  /** how many messages are read for each text, the best of them are listed */
  const HITS_READ = 20;

  const ALL = "all";
  const NEW = "new";
  const JOIN = "join";

  let typed = $derived(q);
  let open = $state(false);

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
  /** the hard result when there is one, else every result */
  const first = $derived(listed.hard ? hardValue(listed.hard) : ALL);

  // the text is read once typing pauses
  $effect(() => {
    void text;
    const timer = setTimeout(() => {
      if (!box.current) void box.read();
    }, TYPING_PAUSE);
    return () => clearTimeout(timer);
  });

  /** Opens a chat the box named: the jump is done, so the text goes with it */
  const openChat = (href: string) => {
    typed = "";
    void goto(href);
  };

  const choose = (value: string) => {
    const hit = listed.hits.find((h) => hitValue(h) === value);
    if (hit) void goto(foundHref(hitTarget(hit), text, "all", hit.localId));
    // every message, as the dropdown showed; which of them the results show is the viewer's filter
    else if (value === ALL) {
      const to = resultsHref(text);
      // the results already on screen take no second step in the history
      void goto(to, { replaceState: to === page.url.hash });
    } else if (value === NEW) onnew(text);
    else if (value === JOIN) onjoin(text);
    else if (value.startsWith("room:")) openChat(roomHref(value.slice(5)));
    else if (value.startsWith("user:")) openChat(chatHref(value.slice(5)));
  };
</script>

{#snippet everything()}
  <Suggest.Item value={ALL} icon={SearchIcon}>All results for "{text}"</Suggest.Item>
{/snippet}

<Suggest.Box
  bind:value={typed}
  bind:open
  {first}
  settle={box.settled}
  onchoose={choose}
  icon={SearchIcon}
  type="search"
  autocomplete="off"
  maxlength={MAX_TEXT}
  placeholder="Rooms, users and messages"
  aria-label="Search chats">
  <!-- a new first entry takes the highlight: entries made anew move it to the first -->
  {#if listed.hard}
    {@const hard = listed.hard}
    {#key first}
      <Suggest.Item value={first}>
        {#if hard.kind === "room"}
          <RoomFound room={hard.room} {text} />
        {:else}
          <UserEntry user={hard.user} {text} />
        {/if}
      </Suggest.Item>
    {/key}
  {:else}
    {@render everything()}
  {/if}
  {#if listed.rooms.length}
    <Suggest.Group heading="Rooms">
      {#each listed.rooms as room (room.name)}
        <Suggest.Item value={roomValue(room.name)}><RoomFound {room} {text} /></Suggest.Item>
      {/each}
    </Suggest.Group>
  {/if}
  {#if listed.users.length}
    <Suggest.Group heading="Users">
      {#each listed.users as user (user.name)}
        <Suggest.Item value={userValue(user.name)}><UserEntry {user} {text} /></Suggest.Item>
      {/each}
    </Suggest.Group>
  {/if}
  {#if listed.hits.length}
    <Suggest.Group heading="Messages">
      {#each listed.hits as hit (hitValue(hit))}
        {@const chat = hitTarget(hit)}
        <Suggest.Item value={hitValue(hit)}>
          <span class="flex min-w-0 flex-1 flex-col gap-0.5">
            <span class="flex items-baseline gap-2 text-xs text-fg-muted">
              <span class="min-w-0 truncate font-medium">
                {chat.kind === "room" ? `#${chat.name}, ${hit.username}` : chatName(chat)}
              </span>
              <Tooltip text={dateTime(hit.timestamp)} dense>
                {#snippet child({ props })}
                  <time {...props} class="ml-auto shrink-0" datetime={hit.timestamp}
                    >{ago(hit.timestamp, clock.now)}</time>
                {/snippet}
              </Tooltip>
            </span>
            <span class="line-clamp-2 wrap-anywhere" dir="auto"
              ><Marked parts={snippetParts(hit.snippet)} /></span>
          </span>
        </Suggest.Item>
      {/each}
    </Suggest.Group>
  {/if}
  {#if noName && !listed.hits.length && box.current}
    <p class="px-2 py-1 text-sm text-fg-muted">No rooms, users or messages match</p>
  {/if}
  {#if noName && write}
    {#if text !== SERVER}
      <Suggest.Item value={NEW} icon={MessageSquarePlusIcon}>
        Message <span class="font-medium">{text}</span>
      </Suggest.Item>
    {/if}
    <Suggest.Item value={JOIN} icon={UsersRoundIcon}>
      Join <span class="font-medium">{text}</span>
    </Suggest.Item>
  {/if}
  {#if listed.hard}{@render everything()}{/if}
</Suggest.Box>
