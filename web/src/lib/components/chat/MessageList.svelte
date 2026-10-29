<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import XIcon from "@lucide/svelte/icons/x";
  import { defaultRangeExtractor } from "@tanstack/virtual-core";
  import { tick, untrack } from "svelte";
  import { fly } from "svelte/transition";

  import { type Line, layout, LONG, type Message } from "#lib/chat/messages.js";
  import { pullOlder } from "#lib/chat/older.js";
  import type { MessagePane } from "#lib/chat/pane.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { formatDay, plural } from "#lib/format.js";
  import { freezesWhileUsed, interaction } from "#lib/lists/interaction.svelte.js";
  import { ListWindow } from "#lib/lists/virtual.svelte.js";
  import { motionFast } from "#lib/motion.js";

  import GapRow from "./GapRow.svelte";
  import MessageRow from "./MessageRow.svelte";

  type Props = {
    pane: MessagePane;
    /** a message to scroll to and mark, from a link */
    at: number | null;
    /** the words marked in the messages: our name and the watched words */
    watch: string[];
    /** quotes a message in the composer, when one can be sent */
    onreply?: (message: Message) => void;
    /** the newest messages are in view on a visible tab: they count as read */
    onread: () => void;
  };

  let { pane, at, watch, onreply, onread }: Props = $props();

  /** this close to the bottom counts as at the bottom */
  const NEAR = 48;

  /** the line at the top of a history read back to its start */
  type Start = { kind: "start"; key: "start" };

  let box = $state<HTMLDivElement | null>(null);
  let atBottom = $state(true);
  /** messages that arrived while scrolled up, for the pill */
  let fresh = $state(0);
  let loadingOlder = $state(false);
  /** the scrollbar is held down: a press on the list beside its content */
  let gripped = $state(false);
  let placed = false;
  /** the newest message when the list was placed or last followed a new one */
  let seenNewest: number | null = null;

  const lines = $derived<(Line | Start)[]>([
    ...(!pane.hasMore && pane.loaded && pane.ordered.length
      ? [{ kind: "start", key: "start" } as const]
      : []),
    ...layout(pane.ordered, pane.gaps, pane.unreadFrom)
  ]);
  const newest = $derived(pane.ordered.at(-1)?.localId ?? null);

  /**
   * A line's height before it is drawn. A message counts one 20 px line per 80 characters of
   * each line of its text, plus 2 px of padding, or 32 px with the header of a group's first
   */
  const guess = (line: Line | Start) => {
    if (line.kind === "unread") return 24;
    if (line.kind !== "message") return 36;
    const { message: m, head } = line;
    const rows = m.hidden
      ? 1
      : m.text
          .slice(0, LONG)
          .split("\n")
          .reduce((n, text) => n + Math.max(1, Math.ceil(text.length / 80)), 0);
    return (head ? 32 : 2) + 20 * rows;
  };

  /** the lines drawn when the user took hold of the list, which stay drawn while it is held */
  let held: string[] = [];
  /*
   * The list is windowed: only the lines near the view are in the page. A line's height is
   * guessed before it is drawn and measured once it is. Older messages arriving above leave the
   * first message in view where it was; at the bottom, new ones are followed unless the user
   * holds the list. While the user holds it (a selection, a press), the lines drawn when they
   * took hold stay drawn, so nothing selected leaves the page
   */
  const shown = new ListWindow(
    () => lines,
    (line) => line.key,
    () => ({
      estimateSize: (index) => guess(lines[index]),
      overscan: 12,
      paddingEnd: 8,
      // while the scrollbar is held at the top, older pages go above it rather than keep the view
      anchor: gripped ? undefined : (line) => line.kind === "message",
      followOnAppend: !interaction.frozen,
      anchorTo: "end",
      scrollEndThreshold: NEAR,
      rangeExtractor: (range) => {
        const drawn = defaultRangeExtractor(range);
        if (!interaction.frozen || !held.length) return drawn;
        const keys = new Set(drawn.map((index) => lines[index]?.key));
        const kept = held.flatMap((key) => {
          const index = lines.findIndex((l) => l.key === key);
          return index === -1 || keys.has(key) ? [] : [index];
        });
        return [...drawn, ...kept].toSorted((a, b) => a - b);
      }
    })
  );
  $effect(() => {
    const keys = shown.drawn.map((d) => d.key);
    if (!interaction.frozen) held = keys;
  });

  /** Reads whether the view is at the bottom */
  const seen = () => {
    if (box) atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < NEAR;
  };
  const toBottom = () => {
    shown.scrollToEnd();
    fresh = 0;
  };
  /** where the line marking the first unread message is, -1 without one */
  const unreadAt = $derived(lines.findIndex((l) => l.kind === "unread"));
  /** how many messages came after the unread line */
  const unreadCount = $derived(
    unreadAt === -1 ? 0 : lines.slice(unreadAt).filter((l) => l.kind === "message").length
  );
  let unreadDismissed = $state(false);
  /** the unread line is above the view: a pill offers to go up to it */
  const unreadAbove = $derived(
    !unreadDismissed && unreadAt !== -1 && (shown.startOf(unreadAt) ?? 0) < shown.offset
  );
  const toUnread = () => {
    if (unreadAt === -1) return;
    shown.scrollTo(unreadAt, "start");
    unreadDismissed = true;
  };

  /** The newest messages count as read only when the view really is at them on a visible tab */
  const readIfSeen = () => {
    seen();
    if (atBottom && document.visibilityState === "visible") onread();
  };

  // the first page shows at the bottom, or at the message a link names
  $effect(() => {
    if (!pane.loaded || placed) return;
    placed = true;
    seenNewest = untrack(() => newest);
    untrack(async () => {
      if (at !== null) await pane.reach(at);
      await tick();
      const index = at === null ? -1 : lines.findIndex((l) => l.key === `m:${at}`);
      if (index === -1) toBottom();
      else shown.scrollTo(index, "center");
      await tick();
      readIfSeen();
    });
  });

  // a new message is followed at the bottom when the view was there and nothing holds the list;
  // otherwise the pill counts it
  $effect(() => {
    if (newest === null) return;
    untrack(async () => {
      // the first page is placed on its own; this follows the messages after it
      if (!placed || newest === seenNewest) return;
      seenNewest = newest;
      if (atBottom && !interaction.frozen) {
        await tick();
        readIfSeen();
      } else fresh += 1;
    });
  });

  const scrolled = async () => {
    if (!box) return;
    seen();
    if (atBottom) {
      fresh = 0;
      readIfSeen();
    }
    if (box.scrollTop < NEAR * 2 && pane.hasMore && !loadingOlder && placed) {
      // older messages go above; the view stays on the message it showed, or at the top for as
      // long as the scrollbar is held there
      loadingOlder = true;
      await pullOlder({
        older: () => pane.older(),
        more: () => pane.hasMore,
        held: () => gripped && !!box && box.scrollTop < NEAR * 2,
        settle: async (holding) => {
          await tick();
          if (holding && box) box.scrollTop = 0;
        }
      });
      loadingOlder = false;
    }
  };

  const grip = (e: PointerEvent) => {
    if (box && e.target === box && e.offsetX >= box.clientWidth) gripped = true;
  };
</script>

<svelte:document onvisibilitychange={readIfSeen} />
<svelte:window onpointerup={() => (gripped = false)} onpointercancel={() => (gripped = false)} />

<div class="relative flex min-h-0 flex-1 flex-col">
  <div
    bind:this={box}
    class="relative min-h-0 flex-1 overflow-y-auto [overflow-anchor:none]"
    role="log"
    aria-live="polite"
    onscroll={scrolled}
    onpointerdown={grip}
    {@attach shown.attach}
    {@attach freezesWhileUsed}>
    <div class="relative" style:height={`${shown.total}px`}>
      {#each shown.drawn as { line, index, start } (line.key)}
        <div
          class="absolute inset-x-0 top-0"
          style:transform={`translateY(${start}px)`}
          data-index={index}
          data-key={line.key}
          {@attach shown.measure}>
          {#if line.kind === "start"}
            <p class="py-2 text-center text-xs text-fg-subtle">The start of the stored history</p>
          {:else if line.kind === "day"}
            <div class="flex items-center gap-3 px-3 py-2 text-xs text-fg-muted" role="separator">
              <span class="h-px flex-1 bg-border"></span>
              {formatDay(line.date)}
              <span class="h-px flex-1 bg-border"></span>
            </div>
          {:else if line.kind === "gap"}
            <GapRow gap={line.gap} />
          {:else if line.kind === "unread"}
            <div class="flex items-center gap-3 px-3 py-1 text-xs text-accent" role="separator">
              <span class="h-px flex-1 bg-accent"></span>New<span class="h-px flex-1 bg-accent"
              ></span>
            </div>
          {:else}
            <MessageRow
              message={line.message}
              head={line.head}
              highlight={line.message.localId === at}
              {watch}
              room={pane.target.kind === "room"}
              {onreply}
              onretry={() => void pane.retry(line.message)} />
          {/if}
        </div>
      {/each}
    </div>
  </div>
  {#if unreadAbove}
    <div
      class="absolute inset-x-0 top-2 z-10 flex justify-center"
      transition:fly={{ y: -8, duration: motionFast() }}>
      <div
        class="flex items-center rounded-full border border-accent/40 bg-overlay text-xs shadow-raised">
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-l-full py-1 pr-2 pl-3 font-medium text-accent hover:bg-hover"
          onclick={toUnread}>
          <ArrowUpIcon class="size-3.5" />{plural(unreadCount, "new message", "new messages")}
        </button>
        <button
          type="button"
          class="rounded-r-full border-l px-2 py-1 text-fg-muted hover:bg-hover"
          aria-label="Dismiss"
          onclick={() => (unreadDismissed = true)}>
          <XIcon class="size-3.5" />
        </button>
      </div>
    </div>
  {/if}
  {#if loadingOlder}
    <!-- over the list, not in it: a line in the list would move the messages under the view -->
    <div class="pointer-events-none absolute inset-x-0 top-2 flex justify-center">
      <Spinner class="size-4" />
    </div>
  {/if}
  {#if !atBottom}
    <Button
      size="sm"
      variant="secondary"
      class="absolute bottom-3 left-1/2 -translate-x-1/2 shadow-raised"
      onclick={toBottom}>
      <ArrowDownIcon />{fresh ? `${fresh} new` : "Jump to latest"}
    </Button>
  {/if}
</div>
