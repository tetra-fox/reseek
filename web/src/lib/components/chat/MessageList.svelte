<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import { tick, untrack } from "svelte";

  import { layout } from "#lib/chat/messages.js";
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

  let box = $state<HTMLDivElement | null>(null);
  let atBottom = $state(true);
  /** messages that arrived while scrolled up, for the pill */
  let fresh = $state(0);
  let loadingOlder = $state(false);
  let placed = false;

  const lines = $derived(layout(pane.ordered, pane.gaps, pane.unreadFrom));
  const newest = $derived(pane.ordered.at(-1)?.localId ?? null);

  const toBottom = () => {
    if (box) box.scrollTop = box.scrollHeight;
    fresh = 0;
  };
  const readIfSeen = () => {
    if (atBottom && document.visibilityState === "visible") onread();
  };

  // the first page shows at the bottom, or at the message a link names
  $effect(() => {
    if (!pane.loaded || placed) return;
    placed = true;
    untrack(async () => {
      if (at !== null) await pane.reach(at);
      await tick();
      const target = at === null ? null : box?.querySelector(`[data-message="${at}"]`);
      if (target) target.scrollIntoView({ block: "center" });
      else toBottom();
      readIfSeen();
    });
  });

  // a new message keeps the view at the bottom when it was there and nothing holds the list;
  // otherwise the pill counts it
  $effect(() => {
    if (newest === null) return;
    untrack(async () => {
      if (!placed) return;
      if (atBottom && !interaction.frozen) {
        await tick();
        toBottom();
        readIfSeen();
      } else fresh += 1;
    });
  });

  const scrolled = async () => {
    if (!box) return;
    atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < NEAR;
    if (atBottom) {
      fresh = 0;
      readIfSeen();
    }
    if (box.scrollTop < NEAR * 2 && pane.hasMore && !loadingOlder && placed) {
      // older messages go above; the view stays on the message it showed
      loadingOlder = true;
      const before = box.scrollHeight;
      await pane.older();
      await tick();
      box.scrollTop += box.scrollHeight - before;
      loadingOlder = false;
    }
  };
</script>

<svelte:document onvisibilitychange={readIfSeen} />

<div class="relative flex min-h-0 flex-1 flex-col">
  <div
    bind:this={box}
    class="flex min-h-0 flex-1 flex-col overflow-y-auto pb-2 [overflow-anchor:auto]"
    role="log"
    aria-live="polite"
    onscroll={scrolled}
    {@attach freezesWhileUsed}>
    {#if loadingOlder}
      <div class="flex justify-center py-2"><Spinner class="size-4" /></div>
    {:else if !pane.hasMore && pane.loaded && pane.ordered.length}
      <p class="py-2 text-center text-xs text-fg-subtle">The start of the stored history</p>
    {/if}
    {#each lines as line (line.key)}
      {#if line.kind === "day"}
        <div class="my-2 flex items-center gap-3 px-3 text-xs text-fg-muted" role="separator">
          <span class="h-px flex-1 bg-border"></span>
          {formatDay(line.date)}
          <span class="h-px flex-1 bg-border"></span>
        </div>
      {:else if line.kind === "gap"}
        <GapRow gap={line.gap} />
      {:else if line.kind === "unread"}
        <div class="my-1 flex items-center gap-3 px-3 text-xs text-accent" role="separator">
          <span class="h-px flex-1 bg-accent"></span>New<span class="h-px flex-1 bg-accent"></span>
        </div>
      {:else}
        <MessageRow
          message={line.message}
          head={line.head}
          highlight={line.message.localId === at}
          onretry={() => void pane.retry(line.message)} />
      {/if}
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
</div>
