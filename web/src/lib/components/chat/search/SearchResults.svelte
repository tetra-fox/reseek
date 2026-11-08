<script lang="ts">
  import { goto } from "$app/navigation";
  import HashIcon from "@lucide/svelte/icons/hash";
  import MessageSquarePlusIcon from "@lucide/svelte/icons/message-square-plus";
  import SearchIcon from "@lucide/svelte/icons/search";
  import ServerIcon from "@lucide/svelte/icons/server";
  import UsersRoundIcon from "@lucide/svelte/icons/users-round";
  import type { Snippet } from "svelte";

  import { chatName, isNotices, SERVER } from "#lib/chat/notices.js";
  import type { Target } from "#lib/chat/pane.svelte.js";
  import { foundHref, resultsHref } from "#lib/chat/route.js";
  import {
    groupHits,
    hitTarget,
    type MessageHit,
    type RoomFound as Room,
    type SearchKind
  } from "#lib/chat/search/match.js";
  import type { ChatSearch } from "#lib/chat/search/search.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Marked from "#lib/components/ui/Marked.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import UserEntry from "#lib/components/users/UserEntry.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatCount, formatDate, plural } from "#lib/format.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { snippetParts } from "#lib/text.js";
  import type { UserFound as User } from "#lib/users/match.js";

  import RoomFound from "./RoomFound.svelte";

  type Props = {
    /** what is found for `q`, kept while the chat screen is open */
    search: ChatSearch;
    q: string;
    /** the messages searched, from the address */
    kind: SearchKind;
    /** shown before the title, such as the button that opens the chat list on a narrow screen */
    lead: Snippet;
    onjoin: (name: string) => void;
    onnew: (username: string) => void;
  };

  let { search, q, kind, lead, onjoin, onnew }: Props = $props();

  /** how many rooms and users show until all of them are asked for */
  const SHOWN = 20;

  type Category = "rooms" | "users";
  type Line =
    | { kind: "heading"; key: string; title: string; count: number }
    | { kind: "more"; key: string; of: Category; count: number }
    | { kind: "chat"; key: string; target: Target }
    | { kind: "room"; key: string; room: Room }
    | { kind: "user"; key: string; user: User }
    | { kind: "hit"; key: string; hit: MessageHit };

  $effect(() => {
    if (!search.current) void search.read();
  });

  const lines = $derived.by((): Line[] => {
    const { rooms, users } = search.found;
    const heading = (title: string, count: number): Line[] =>
      count ? [{ kind: "heading", key: `h:${title}`, title, count }] : [];
    const some = <T>(of: Category, all: T[]) => (search.whole[of] ? all : all.slice(0, SHOWN));
    const rest = (of: Category, count: number): Line[] =>
      !search.whole[of] && count > SHOWN ? [{ kind: "more", key: `more:${of}`, of, count }] : [];
    return [
      ...heading("Rooms", rooms.length),
      ...some("rooms", rooms).map((room): Line => ({
        kind: "room",
        key: `room:${room.name}`,
        room
      })),
      ...rest("rooms", rooms.length),
      ...heading("Users", users.length),
      ...some("users", users).map((user): Line => ({
        kind: "user",
        key: `user:${user.name}`,
        user
      })),
      ...rest("users", users.length),
      ...heading("Messages", search.total),
      ...groupHits(search.hits).flatMap(({ key, target, hits }): Line[] => [
        { kind: "chat", key: `chat:${key}`, target },
        ...hits.map((hit): Line => ({ kind: "hit", key: `hit:${key}:${hit.localId}`, hit }))
      ])
    ];
  });

  const open = (line: Line) => {
    if (line.kind === "more") return void (search.whole[line.of] = true);
    search.line = line.key;
    if (line.kind === "room") void goto(foundHref({ kind: "room", name: line.room.name }, q, kind));
    else if (line.kind === "user") {
      void goto(foundHref({ kind: "user", name: line.user.name }, q, kind));
    } else if (line.kind === "hit") {
      void goto(foundHref(hitTarget(line.hit), q, kind, line.hit.localId));
    }
  };

  const list = new ListControl<Line>({
    lines: () => lines,
    key: (l) => l.key,
    id: (l) => (l.kind === "heading" || l.kind === "chat" ? null : l.key),
    onopen: open,
    openOnClick: true
  });
  // svelte-ignore state_referenced_locally
  list.focusedKey = search.line;

  /** Puts the view back where it was and takes the keys, on arrival from the box or a chat */
  const arrive = (node: HTMLElement) => {
    node.scrollTop = search.scrollTop;
    list.focus();
  };

  const write = $derived(session.can("ReadWrite"));
  const empty = $derived(!lines.length && !search.error);
  const row = "flex items-center gap-2 pick-row px-2 py-1.5";
</script>

<div class="flex min-h-0 flex-1 flex-col">
  <header class="flex flex-wrap items-center gap-x-2 gap-y-1 border-b px-3 py-2">
    {@render lead()}
    <h2 class="min-w-0 flex-1 basis-40 truncate font-medium">
      {q ? `Results for "${q}"` : "Search"}
    </h2>
    {#if q}
      <Select
        class="w-44"
        aria-label="Which messages"
        bind:value={() => kind, (next) => void goto(resultsHref(q, next), { replaceState: true })}
        options={[
          { value: "all" as SearchKind, label: "All messages" },
          { value: "private" as SearchKind, label: "Private messages" },
          { value: "rooms" as SearchKind, label: "Room messages" }
        ]} />
    {/if}
  </header>
  {#if search.error}<Notice kind="error">{search.error}</Notice>{/if}
  {#if !q}
    <EmptyState
      icon={SearchIcon}
      title="Search rooms, users and messages"
      description={'Use the box above the chat list. Messages match a "quoted phrase" or one word OR another.'} />
  {:else if !search.current}
    <div class="flex flex-col gap-3 p-3" aria-hidden="true">
      {#each [0, 1, 2] as n (n)}<Skeleton class="h-9 w-full" />{/each}
    </div>
  {:else if empty}
    <EmptyState icon={SearchIcon} title={`Nothing matches "${q}"`}>
      {#snippet action()}
        {#if write}
          <div class="flex flex-wrap justify-center gap-1">
            {#if q !== SERVER}
              <Button variant="outline" size="sm" onclick={() => onnew(q)}
                ><MessageSquarePlusIcon />Message {q}</Button>
            {/if}
            <Button variant="outline" size="sm" onclick={() => onjoin(q)}
              ><UsersRoundIcon />Join {q}</Button>
          </div>
        {/if}
      {/snippet}
    </EmptyState>
  {:else}
    <div
      class="min-h-0 flex-1 overflow-y-auto p-2 outline-none"
      role="grid"
      aria-label="Search results"
      aria-activedescendant={list.activeId}
      tabindex="0"
      onscroll={(e) => (search.scrollTop = e.currentTarget.scrollTop)}
      {@attach list.attach}
      {@attach arrive}>
      {#each lines as line, i (line.key)}
        {#if line.kind === "heading"}
          <div role="row" {...list.line(line, i)} class="flex gap-2 px-2 pt-3 pb-1 text-sm">
            <span role="columnheader" class="font-medium">{line.title}</span>
            {#if line.title === "Messages"}
              <span class="text-fg-muted">{plural(line.count, "match", "matches")}</span>
            {/if}
          </div>
        {:else if line.kind === "more"}
          <div role="row" {...list.line(line, i)} class={[row, "text-sm text-accent"]}>
            <span role="gridcell">Show all {formatCount(line.count)} {line.of}</span>
          </div>
        {:else if line.kind === "chat"}
          <div
            role="row"
            {...list.line(line, i)}
            class="flex items-center gap-1.5 px-2 pt-2 text-xs font-medium text-fg-muted">
            <span role="gridcell" class="flex min-w-0 items-center gap-1.5">
              {#if line.target.kind === "room"}
                <HashIcon class="size-3.5 shrink-0" /><span class="truncate"
                  >{line.target.name}</span>
              {:else if isNotices(line.target)}
                <ServerIcon class="size-3.5 shrink-0" /><span class="truncate"
                  >{chatName(line.target)}</span>
              {:else}
                <Username name={line.target.name} />
              {/if}
            </span>
          </div>
        {:else if line.kind === "room"}
          <div role="row" {...list.line(line, i)} class={row}>
            <span role="gridcell" class="flex min-w-0 flex-1 items-center gap-2"
              ><RoomFound room={line.room} text={q} /></span>
          </div>
        {:else if line.kind === "user"}
          <div role="row" {...list.line(line, i)} class={row}>
            <span role="gridcell" class="flex min-w-0 flex-1 items-center gap-2"
              ><UserEntry user={line.user} text={q} /></span>
          </div>
        {:else}
          {@const hit = line.hit}
          <div role="row" {...list.line(line, i)} class={[row, "flex-wrap items-baseline"]}>
            <span role="gridcell" class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2">
              <time class="text-xs text-fg-subtle" datetime={hit.timestamp}
                >{formatDate(hit.timestamp)}</time>
              {#if hit.room}<Username name={hit.username} class="text-xs" />{/if}
              <span class="min-w-0 flex-1 basis-48 text-sm wrap-anywhere" dir="auto"
                ><Marked parts={snippetParts(hit.snippet)} /></span>
            </span>
          </div>
        {/if}
      {/each}
      {#if search.hits.length < search.total}
        <div class="flex justify-center p-2">
          <Button variant="outline" onclick={() => void search.more()}>Show more</Button>
        </div>
      {/if}
    </div>
  {/if}
</div>
