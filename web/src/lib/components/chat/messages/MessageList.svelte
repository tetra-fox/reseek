<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import XIcon from "@lucide/svelte/icons/x";
  import { defaultRangeExtractor } from "@tanstack/virtual-core";
  import { tick, untrack } from "svelte";
  import { fly } from "svelte/transition";

  import {
    actionable,
    type Line,
    layout,
    LONG,
    type Message
  } from "#lib/chat/messages/messages.js";
  import { pullOlder } from "#lib/chat/messages/older.js";
  import { isNotices } from "#lib/chat/notices.js";
  import type { MessagePane } from "#lib/chat/pane.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { formatDay, plural } from "#lib/format.js";
  import { freezesWhileUsed, interaction } from "#lib/lists/interaction.svelte.js";
  import { ListWindow } from "#lib/lists/virtual.svelte.js";
  import { motionFast } from "#lib/ui/motion.js";
  import { rem } from "#lib/ui/viewport.svelte.js";

  import GapRow from "./GapRow.svelte";
  import MessageRow from "./MessageRow.svelte";

  type Props = {
    pane: MessagePane;
    /** the words marked in the messages: our name and the watched words */
    watch: string[];
    /** quotes a message in the composer, when one can be sent */
    onreply?: (message: Message) => void;
    /** the newest messages are in view on a visible tab: they count as read */
    onread: () => void;
  };

  let { pane, watch, onreply, onread }: Props = $props();

  const notices = $derived(isNotices(pane.target));

  /** this close to the bottom counts as at the bottom */
  const NEAR = 48;

  /** the line at the top of a history read back to its start */
  type Start = { kind: "start"; key: "start" };

  let box = $state<HTMLDivElement | null>(null);
  let atBottom = $state(true);
  /** messages that arrived while scrolled up, for the pill */
  let fresh = $state(0);
  let loadingOlder = $state(false);
  let loadingNewer = $state(false);
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
   * A line's height before it is drawn, in rem. A line drawn at another height than its guess
   * moves the lines under it, so the separators' guesses are their drawn heights: one line of
   * small text, with `py-2` for the start, a day and a gap
   */
  const guess = (line: Line | Start) => {
    if (line.kind === "unread") return 1.5;
    if (line.kind !== "message") return 2;
    const { message: m, head } = line;
    const rows = m.hidden
      ? 1
      : m.text
          .slice(0, LONG)
          .split("\n")
          .reduce((n, text) => n + Math.max(1, Math.ceil(text.length / 80)), 0);
    return (head ? 2 : 0.125) + 1.25 * rows;
  };

  /** the message the arrow keys are on, the one message that can take focus */
  let active = $state<number | null>(null);
  /** the message whose menu is open */
  let menuFor = $state<number | null>(null);
  /** where the active message is in the lines, -1 when none is */
  const activeAt = $derived(
    active === null
      ? -1
      : lines.findIndex((l) => l.kind === "message" && l.message.localId === active)
  );

  /** the lines drawn when the user took hold of the list, which stay drawn while it is held */
  let held: string[] = [];
  /*
   * The list is windowed: only the lines near the view are in the page. A line's height is
   * guessed before it is drawn and measured once it is. Older messages arriving above leave the
   * first message in view where it was; at the bottom, new ones are followed unless the user
   * holds the list. While the user holds it (a selection, a press), the lines drawn when they
   * took hold stay drawn, so nothing selected leaves the page. The active message stays drawn
   * too, so focus on it is never dropped with it
   */
  const shown = new ListWindow(
    () => lines,
    (line) => line.key,
    () => {
      const keep = activeAt;
      return {
        estimateSize: (index) => guess(lines[index]) * rem.px,
        sizes: rem.px,
        overscan: 12,
        paddingEnd: 8,
        // while the scrollbar is held at the top, older pages go above it rather than keep the view
        anchor: gripped ? undefined : (line) => line.kind === "message",
        // a page read on the way down stays below the view, which is reading older messages
        followOnAppend: !interaction.frozen && !pane.hasNewer && !loadingNewer,
        anchorTo: "end",
        scrollEndThreshold: NEAR,
        rangeExtractor: (range) => {
          const drawn = new Set(defaultRangeExtractor(range));
          if (keep !== -1) drawn.add(keep);
          if (interaction.frozen) {
            for (const key of held) {
              const index = lines.findIndex((l) => l.key === key);
              if (index !== -1) drawn.add(index);
            }
          }
          return [...drawn].toSorted((a, b) => a - b);
        }
      };
    }
  );
  $effect(() => {
    const keys = shown.drawn.map((d) => d.key);
    if (!interaction.frozen) held = keys;
  });

  // the lines stay as they are while a message's menu hangs from one
  $effect(() => {
    if (menuFor !== null) return interaction.hold();
  });

  /** The active message's element, while it is drawn */
  const activeRow = () =>
    active === null ? null : box?.querySelector<HTMLElement>(`[data-message="${active}"]`);

  /** Gives the active message DOM focus once it is drawn, so Tab goes on to its actions */
  const focusActive = async () => {
    await tick();
    activeRow()?.focus({ preventScroll: true });
  };

  // the lines can lose the active message, as when the view is cleared or goes to the newest
  // messages: before its row leaves, focus on it goes back to the log
  $effect.pre(() => {
    if (active === null || activeAt !== -1) return;
    if (activeRow()?.contains(document.activeElement)) box?.focus({ preventScroll: true });
    active = null;
  });

  /**
   * The arrows walk the messages with an action, from the newest drawn, and focus each; Enter
   * opens the menu. Keys on a control inside a message are the control's
   */
  const keydown = (e: KeyboardEvent) => {
    if (e.target !== box && e.target !== activeRow()) return;
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const indexes = lines.flatMap((line, i) =>
        line.kind === "message" && actionable(line.message) ? [i] : []
      );
      const idOf = (i: number) => (lines[i] as Line & { kind: "message" }).message.localId;
      const at = indexes.findIndex((i) => idOf(i) === active);
      const from = at === -1 ? indexes.findLastIndex((i) => shown.isDrawn(i)) : at;
      const step = at === -1 ? 0 : e.key === "ArrowUp" ? -1 : 1;
      const to = indexes[Math.min(Math.max(from + step, 0), indexes.length - 1)];
      if (to === undefined) return;
      active = idOf(to);
      shown.scrollTo(to);
      void focusActive();
    } else if (e.key === "Escape") {
      active = null;
      box?.focus({ preventScroll: true });
    } else if (active !== null) {
      if (e.key !== "Enter" && e.key !== "ContextMenu" && !(e.key === "F10" && e.shiftKey)) return;
      e.preventDefault();
      // the menu hangs from the message, which may have been scrolled away from
      shown.scrollTo(activeAt);
      menuFor = active;
    }
  };

  /**
   * Focus went from the log to something else, not the active message's menu: the keys are off
   * it. Focus that went nowhere, as when the window is left, comes back to the message
   */
  const focusout = (e: FocusEvent) => {
    const to = e.relatedTarget;
    if (menuFor === null && to instanceof Node && !box?.contains(to)) active = null;
  };

  /** Reads whether the view is at the bottom */
  const seen = () => {
    if (box) atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < NEAR;
  };
  const toBottom = () => {
    shown.scrollToEnd();
    fresh = 0;
  };
  /**
   * Keeps a view that follows the newest message on it as a change of width wraps the messages
   * taller, which moves no scroll position and so would push the newest out of view. A page read
   * on the way down grows the list as well, and stays below the view
   */
  const followsEnd = (node: HTMLElement) => {
    const observer = new ResizeObserver(() => {
      if (placed && atBottom && !loadingNewer && !interaction.frozen) toBottom();
    });
    observer.observe(node);
    if (node.firstElementChild) observer.observe(node.firstElementChild);
    return () => observer.disconnect();
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
    if (atBottom && !pane.hasNewer && document.visibilityState === "visible") onread();
  };

  // a message unread again once the list is placed, as one the server delivered again, is read
  // again while the view is at the newest; the first page's unread ones wait for their placing
  const unreadHeld = $derived(pane.ordered.some((m) => m.unread));
  $effect(() => {
    if (unreadHeld && placed) untrack(readIfSeen);
  });

  // the first page shows at the bottom, or at the message a link names; one no longer stored
  // shows where it was, at the first message after it
  $effect(() => {
    if (!pane.loaded || placed) return;
    placed = true;
    seenNewest = untrack(() => newest);
    untrack(async () => {
      await tick();
      const at = pane.at;
      const index =
        at === null ? -1 : lines.findIndex((l) => l.kind === "message" && l.message.localId >= at);
      const line = lines[index];
      if (at !== null && (line?.kind !== "message" || line.message.localId !== at)) {
        toast("That message is no longer stored");
      }
      if (index === -1) toBottom();
      else shown.scrollTo(index, "center");
      await tick();
      readIfSeen();
    });
  });

  // back at the newest messages, from the pill or by sending, the view goes to the bottom
  $effect(() => {
    if (!pane.returned) return;
    untrack(async () => {
      await tick();
      toBottom();
      await tick();
      readIfSeen();
    });
  });

  // a new message is followed at the bottom when the view was there and nothing holds the list,
  // and the oldest messages past what a pane keeps go; otherwise the pill counts it
  $effect(() => {
    if (newest === null) return;
    untrack(async () => {
      // the first page is placed on its own; this follows the messages after it
      if (!placed || newest === seenNewest) return;
      seenNewest = newest;
      // a page read on the way down is history, not news
      if (loadingNewer) return;
      if (atBottom && !interaction.frozen) {
        pane.trim();
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
    const left = box.scrollHeight - box.scrollTop - box.clientHeight;
    if (left < NEAR * 2 && pane.hasNewer && !loadingNewer && placed) {
      loadingNewer = true;
      await pane.newer();
      await tick();
      // the page is below the view now, which is no longer at the bottom
      seen();
      loadingNewer = false;
    }
  };

  const grip = (e: PointerEvent) => {
    if (box && e.target === box && e.offsetX >= box.clientWidth) gripped = true;
  };
</script>

<svelte:document onvisibilitychange={readIfSeen} />
<svelte:window onpointerup={() => (gripped = false)} onpointercancel={() => (gripped = false)} />

<div class="relative flex min-h-0 flex-1 flex-col">
  <!-- a log takes focus: it scrolls, and the arrow keys walk its messages from there -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={box}
    class="relative min-h-0 flex-1 overflow-y-auto outline-none [overflow-anchor:none] focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
    role="log"
    aria-live="polite"
    aria-label="Messages"
    aria-keyshortcuts="ArrowUp ArrowDown Enter"
    tabindex="0"
    onscroll={scrolled}
    onpointerdown={grip}
    onkeydown={keydown}
    onfocusout={focusout}
    {@attach shown.attach}
    {@attach followsEnd}
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
              head={line.head && !notices}
              highlight={line.message.localId === pane.at}
              {watch}
              room={pane.target.kind === "room"}
              notice={notices}
              active={line.message.localId === active}
              menu={line.message.localId === menuFor}
              onmenu={(open) => {
                menuFor = open ? line.message.localId : null;
                // the keys go on from the message the menu was opened on
                if (!open && active !== null) void focusActive();
              }}
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
  {#if !atBottom || pane.hasNewer}
    <Button
      size="sm"
      variant="secondary"
      class="absolute bottom-3 left-1/2 -translate-x-1/2 shadow-raised"
      onclick={() => (pane.hasNewer ? void pane.latest() : toBottom())}>
      <ArrowDownIcon />{fresh ? `${fresh} new` : "Jump to latest"}
    </Button>
  {/if}
</div>
