<script lang="ts">
  import { goto } from "$app/navigation";
  import HashIcon from "@lucide/svelte/icons/hash";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { untrack } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatDate } from "#lib/format.js";
  import { chatHref, href, roomHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { snippetParts } from "#lib/text.js";

  // the stored messages that match a full-text query, grouped by room and conversation
  let { q: start }: { q: string } = $props();

  const PAGE = 50;
  type Kind = Schemas["MessageSearchKind"];

  // svelte-ignore state_referenced_locally
  let typed = $state(start);
  let kind = $state<Kind>("all");
  let hits = $state.raw<Schemas["MessageHit"][]>([]);
  let total = $state(0);
  let error = $state("");
  let searched = $state(false);

  const run = async (q: string, offset: number) => {
    if (!q.trim()) return;
    const outcome = await call(() =>
      api.GET("/api/v0/messages/search", {
        params: { query: { q: q.trim(), kind, offset, limit: PAGE } }
      })
    );
    searched = true;
    if (!outcome.ok) return void (error = outcome.error);
    error = "";
    hits = offset ? [...hits, ...outcome.data.hits] : outcome.data.hits;
    total = outcome.data.total;
  };

  $effect(() => {
    void kind;
    untrack(() => void run(typed, 0));
  });

  const groups = $derived.by(() => {
    const by = new Map<string, { title: string; room: boolean; hits: Schemas["MessageHit"][] }>();
    for (const hit of hits) {
      const key = hit.kind === "room" ? `room:${hit.room}` : `user:${hit.username}`;
      const group = by.get(key) ?? {
        title: hit.kind === "room" ? (hit.room ?? "") : hit.username,
        room: hit.kind === "room",
        hits: []
      };
      group.hits.push(hit);
      by.set(key, group);
    }
    return [...by.entries()];
  });

  const at = (hit: Schemas["MessageHit"]) =>
    `${hit.kind === "room" ? roomHref(hit.room ?? "") : chatHref(hit.username)}?at=${hit.localId}`;
</script>

<form
  class="flex flex-wrap gap-inline"
  onsubmit={(e) => {
    e.preventDefault();
    void goto(href(`/chat/search?q=${encodeURIComponent(typed.trim())}`), { replaceState: true });
    void run(typed, 0);
  }}>
  <div class="relative min-w-60 flex-1">
    <SearchIcon class="pointer-events-none absolute top-2.5 left-2.5 size-4 text-fg-muted" />
    <Input
      class="pl-8"
      type="search"
      maxlength={200}
      placeholder={'Words, a "quoted phrase", or one OR another'}
      aria-label="Search messages"
      bind:value={typed} />
  </div>
  <Select
    class="w-40"
    aria-label="Where"
    bind:value={kind}
    options={[
      { value: "all" as Kind, label: "Everywhere" },
      { value: "private" as Kind, label: "Conversations" },
      { value: "rooms" as Kind, label: "Rooms" }
    ]} />
  <Button type="submit">Search</Button>
</form>

{#if error}<Notice kind="error">{error}</Notice>{/if}

{#if searched && !hits.length && !error}
  <EmptyState icon={SearchIcon} title="No messages match" />
{/if}

{#each groups as [key, group] (key)}
  <section class="flex flex-col gap-1 rounded-card border bg-surface p-3 shadow-card">
    <h3 class="flex items-center gap-1.5 text-sm font-semibold">
      {#if group.room}<HashIcon class="size-4 text-fg-muted" />{group.title}{:else}<Username
          name={group.title} />{/if}
    </h3>
    <ul class="flex flex-col">
      {#each group.hits as hit (hit.localId)}
        <li class="flex flex-wrap items-baseline gap-x-2 border-t py-1.5 text-sm first:border-t-0">
          <time class="text-xs text-fg-subtle" datetime={hit.timestamp}
            >{formatDate(hit.timestamp)}</time>
          {#if group.room}<span class="text-xs font-medium">{hit.username}</span>{/if}
          <span class="min-w-0 flex-1 cursor-text wrap-anywhere">
            {#each snippetParts(hit.snippet) as part (part.at)}{#if part.hit}<mark
                  class="rounded-sm bg-warning-soft px-0.5 text-fg">{part.text}</mark
                >{:else}{part.text}{/if}{/each}
          </span>
          <a href={at(hit)} class="text-xs text-accent hover:underline">Show in chat</a>
        </li>
      {/each}
    </ul>
  </section>
{/each}

{#if hits.length < total}
  <Button variant="outline" class="self-center" onclick={() => void run(typed, hits.length)}
    >Show more</Button>
{/if}
