<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import { tick, untrack } from "svelte";
  import { on } from "svelte/events";
  import { SvelteMap } from "svelte/reactivity";

  import { type Line, layout } from "#lib/chat/messages.js";
  import type { MessagePane } from "#lib/chat/pane.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { formatDay } from "#lib/format.js";
  import { freezesWhileUsed, interaction } from "#lib/interaction.svelte.js";

  import GapRow from "./GapRow.svelte";
  import MessageRow from "./MessageRow.svelte";

  type Props = {
    pane: MessagePane;
    /** a message to scroll to and mark, from a link */
    at: number | null;
    /** the newest messages are in view on a visible tab: they count as read */
    onread: () => void;
  };

  let { pane, at, onread }: Props = $props();

  /** this close to the bottom counts as at the bottom */
  const NEAR = 48;
  /** lines drawn beyond the visible ones, above and below, in pixels */
  const OVERSCAN = 800;

  let box = $state<HTMLDivElement | null>(null);
  /** the spacer above the drawn lines, where `tops` starts in the box */
  let head = $state<HTMLDivElement | null>(null);
  let atBottom = $state(true);
  /** messages that arrived while scrolled up, for the pill */
  let fresh = $state(0);
  let loadingOlder = $state(false);
  let scrollTop = $state(0);
  let view = $state(0);
  let placed = false;
  /** the newest message when the list was placed or last followed a new one */
  let seenNewest: number | null = null;

  const lines = $derived(layout(pane.ordered, pane.gaps, pane.unreadFrom));
  const newest = $derived(pane.ordered.at(-1)?.localId ?? null);

  /*
   * The list is windowed: only the lines near the view are in the page, between two spacers as
   * tall as the lines above and below. A line's height is measured once it is drawn and guessed
   * before; the browser's scroll anchoring keeps the view still as guesses become measures. While
   * the user holds the list (a selection, a press), the window only grows, so nothing selected
   * leaves the page
   */
  const heights = new SvelteMap<string, number>();
  const guess = (line: Line) =>
    line.kind === "message" ? (line.head ? 52 : 24) : line.kind === "unread" ? 24 : 36;
  const tops = $derived.by(() => {
    const at = [0];
    for (const line of lines) at.push(at.at(-1)! + (heights.get(line.key) ?? guess(line)));
    return at;
  });
  /** the first index whose line ends below `y` */
  const indexAt = (y: number) => {
    let low = 0;
    let high = lines.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (tops[mid + 1] <= y) low = mid + 1;
      else high = mid;
    }
    return low;
  };
  let heldKeys: string[] = [];
  const range = $derived.by(() => {
    let start = indexAt(scrollTop - OVERSCAN);
    let end = Math.min(lines.length, indexAt(scrollTop + view + OVERSCAN) + 1);
    if (interaction.frozen && heldKeys.length) {
      const first = lines.findIndex((l) => l.key === heldKeys[0]);
      const last = lines.findIndex((l) => l.key === heldKeys.at(-1));
      if (first !== -1) start = Math.min(start, first);
      if (last !== -1) end = Math.max(end, last + 1);
    }
    return { start, end };
  });
  const shown = $derived(lines.slice(range.start, range.end));
  $effect(() => {
    heldKeys = shown.map((l) => l.key);
  });

  const observer = new ResizeObserver((entries) => {
    for (const entry of entries) remember(entry.target as HTMLElement);
    // the lines got their real heights: a view kept at the bottom stays there, one kept on a
    // line stays on it
    if (stick && !interaction.frozen) void tick().then(toBottom);
    else if (anchor && !loadingOlder) void tick().then(toAnchor);
  });
  $effect(() => () => observer.disconnect());
  const remember = (node: HTMLElement) => {
    if (node.dataset.key) heights.set(node.dataset.key, node.offsetHeight);
  };
  const measure = (node: HTMLElement) => {
    observer.observe(node);
    return () => observer.unobserve(node);
  };

  /** Reads where the view is, which decides what is drawn and whether it is at the bottom */
  const seen = () => {
    if (!box) return;
    scrollTop = box.scrollTop;
    view = box.clientHeight;
    atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < NEAR;
  };
  /**
   * the view is kept at the bottom: set when it is scrolled there by the list, kept while the
   * lines measured on the way grow the content, and given up when the user scrolls up
   */
  let stick = false;
  /** where the list last scrolled the view to */
  let setTop = 0;
  /**
   * the line the view is kept on while older messages load above it and the lines drawn after
   * get their real heights, with how far below the view's top it was. A scroll while the page
   * loads moves it; one after the page is placed lets it go
   */
  let anchor: { key: string; offset: number } | null = null;
  /**
   * The first message at least partly in view, and where it is. Only a message: a day line
   * takes the key of the day's first message, which changes as older ones arrive
   */
  const anchorOf = () => {
    if (!box) return null;
    for (const node of box.querySelectorAll<HTMLElement>('[data-key^="m:"]')) {
      if (node.offsetTop + node.offsetHeight > box.scrollTop) {
        return { key: node.dataset.key!, offset: node.offsetTop - box.scrollTop };
      }
    }
    return null;
  };
  const toAnchor = () => {
    if (!box || !head || !anchor) return;
    const key = anchor.key;
    const index = lines.findIndex((l) => l.key === key);
    if (index === -1) return;
    box.scrollTop = head.offsetTop + tops[index] - anchor.offset;
    setTop = box.scrollTop;
    seen();
  };
  /**
   * Lets go of the line the view was kept on once the page is placed, when the user scrolls by
   * wheel, touch, key or scrollbar
   */
  const releasesOnUse = (node: HTMLElement) => {
    const release = () => {
      if (!loadingOlder) anchor = null;
    };
    const off = ["wheel", "touchmove", "keydown", "pointerdown"].map((type) =>
      on(node, type, release, { passive: true })
    );
    return () => off.forEach((stop) => stop());
  };
  const toBottom = () => {
    if (!box) return;
    anchor = null;
    stick = true;
    box.scrollTop = box.scrollHeight;
    setTop = box.scrollTop;
    seen();
    fresh = 0;
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
      const index = at === null ? -1 : lines.findIndex((l) => l.key === `m:${at}`);
      if (index === -1 || !box) toBottom();
      else {
        stick = false;
        anchor = null;
        box.scrollTop = Math.max(0, tops[index] - box.clientHeight / 2);
        seen();
        await tick();
        box.querySelector(`[data-message="${at}"]`)?.scrollIntoView({ block: "center" });
      }
      await tick();
      readIfSeen();
    });
  });

  // a new message keeps the view at the bottom when it was there and nothing holds the list;
  // otherwise the pill counts it
  $effect(() => {
    if (newest === null) return;
    untrack(async () => {
      // the first page is placed on its own; this follows the messages after it
      if (!placed || newest === seenNewest) return;
      seenNewest = newest;
      if ((stick || atBottom) && !interaction.frozen) {
        await tick();
        toBottom();
        readIfSeen();
      } else fresh += 1;
    });
  });

  const scrolled = async () => {
    if (!box) return;
    // the content grew under a view kept at the bottom: it follows; a scroll up lets it go
    if (stick && box.scrollTop >= setTop - 1) {
      if (box.scrollHeight - box.scrollTop - box.clientHeight >= NEAR) return toBottom();
    } else stick = false;
    seen();
    if (loadingOlder) return void (anchor = anchorOf());
    if (atBottom) {
      fresh = 0;
      readIfSeen();
    }
    if (box.scrollTop < NEAR * 2 && pane.hasMore && !loadingOlder && placed) {
      // older messages go above; the view stays on the line it showed
      loadingOlder = true;
      anchor = anchorOf();
      await pane.older();
      loadingOlder = false;
      await tick();
      toAnchor();
      // the lines drawn at the new place are measured now rather than when the observer runs, so
      // the view never shows them placed by their guessed heights
      await tick();
      for (const node of box.querySelectorAll<HTMLElement>("[data-key]")) remember(node);
      toAnchor();
    }
  };
</script>

<svelte:document onvisibilitychange={readIfSeen} />

<div class="relative flex min-h-0 flex-1 flex-col">
  <div
    bind:this={box}
    class="relative min-h-0 flex-1 overflow-y-auto pb-2 [overflow-anchor:auto]"
    role="log"
    aria-live="polite"
    onscroll={scrolled}
    {@attach releasesOnUse}
    {@attach freezesWhileUsed}>
    {#if !pane.hasMore && pane.loaded && pane.ordered.length}
      <p class="py-2 text-center text-xs text-fg-subtle">The start of the stored history</p>
    {/if}
    <div bind:this={head} style:height={`${tops[range.start]}px`} aria-hidden="true"></div>
    {#each shown as line (line.key)}
      <div data-key={line.key} {@attach measure}>
        {#if line.kind === "day"}
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
            onretry={() => void pane.retry(line.message)} />
        {/if}
      </div>
    {/each}
    <div style:height={`${tops[lines.length] - tops[range.end]}px`} aria-hidden="true"></div>
  </div>
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
