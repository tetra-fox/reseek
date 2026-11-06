<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import RadioTowerIcon from "@lucide/svelte/icons/radio-tower";
  import SearchIcon from "@lucide/svelte/icons/search";
  import type { Snippet } from "svelte";
  import { tick } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { feedMatches, PublicFeed } from "#lib/chat/rooms/feed.svelte.js";
  import ChatText from "#lib/components/chat/messages/ChatText.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatDate, shortTime } from "#lib/format.js";
  import { interaction } from "#lib/lists/interaction.svelte.js";
  import { roomHref } from "#lib/nav.js";

  /** the button that shows the chat list on a narrow screen */
  let { lead }: { lead?: Snippet } = $props();

  /** how close to the bottom the view counts as there, in pixels */
  const NEAR = 48;

  const feed = new PublicFeed();
  $effect(() => {
    feed.open();
    return () => feed.close();
  });

  let filter = $state("");
  const words = $derived(filter.toLowerCase().split(/\s+/).filter(Boolean));
  const shown = $derived(
    words.length ? feed.lines.filter((line) => feedMatches(line, words)) : feed.lines
  );
  const connected = $derived(app.isServerConnected);

  let box = $state<HTMLElement | null>(null);
  let atBottom = $state(true);
  /** messages shown since the view left the bottom */
  let fresh = $state(0);

  const seen = () => {
    if (box) atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < NEAR;
    if (atBottom) fresh = 0;
  };
  const toBottom = () => {
    if (box) box.scrollTop = box.scrollHeight;
    fresh = 0;
  };

  // a new message is followed at the bottom unless the list is held, as by an open card;
  // otherwise the button counts it
  let count = 0;
  $effect(() => {
    const now = shown.at(-1)?.id ?? 0;
    if (now === count) return;
    count = now;
    if (atBottom && !interaction.frozen) void tick().then(toBottom);
    else fresh += 1;
  });
</script>

<section class="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="All public rooms">
  <header class="flex min-h-12 flex-wrap items-center gap-x-2 gap-y-1 border-b px-3 py-2">
    {@render lead?.()}
    <RadioTowerIcon class="size-4 shrink-0 text-fg-muted" />
    <div class="name-count min-w-0 flex-1 gap-2">
      <h2 class="truncate font-medium">All public rooms</h2>
      <span class="text-xs text-fg-muted"
        ><span>Messages from every public room, shown live and not saved</span></span>
    </div>
    <Input
      type="search"
      icon={SearchIcon}
      bind:value={filter}
      placeholder="Filter by room, name or text"
      aria-label="Filter the messages"
      wrapperClass="w-full sm:w-64" />
  </header>
  <div class="relative flex min-h-0 flex-1 flex-col">
    {#if !shown.length}
      {#if !connected}
        <EmptyState icon={RadioTowerIcon} title="Not connected to the Soulseek server" />
      {:else if feed.lines.length}
        <EmptyState icon={SearchIcon} title="No messages match" />
      {:else}
        <EmptyState icon={RadioTowerIcon} title="Waiting for messages" />
      {/if}
    {:else}
      <!-- a log takes focus, so the keys scroll it -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        bind:this={box}
        class="min-h-0 flex-1 overflow-y-auto py-2 outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
        role="log"
        tabindex="0"
        aria-live="off"
        aria-label="Messages in public rooms"
        onscroll={seen}>
        {#each shown as line (line.id)}
          <div class="flex items-baseline gap-2 px-3 py-0.5 text-sm leading-5">
            <time
              class="shrink-0 text-xs text-fg-subtle tabular-nums"
              datetime={line.receivedAt}
              title={formatDate(line.receivedAt)}>{shortTime(line.receivedAt)}</time>
            <p class="min-w-0 wrap-anywhere">
              <a
                href={roomHref(line.roomName)}
                class="font-medium text-fg-muted underline-offset-2 hover:text-fg hover:underline"
                title={`Open the room ${line.roomName}`}>#{line.roomName}</a>
              <Username name={line.username} card class="ml-1" />
              <span class="ml-1"><ChatText text={line.message} watch={[]} /></span>
            </p>
          </div>
        {/each}
      </div>
      {#if !atBottom}
        <Button
          size="sm"
          variant="secondary"
          class="absolute bottom-3 left-1/2 -translate-x-1/2 shadow-raised"
          onclick={toBottom}>
          <ArrowDownIcon />{fresh ? `${fresh} new` : "Jump to latest"}
        </Button>
      {/if}
    {/if}
  </div>
</section>
