<script lang="ts">
  import ClockIcon from "@lucide/svelte/icons/clock";

  import { LONG, type Message } from "#lib/chat/messages.js";
  import { clock } from "#lib/clock.svelte.js";
  import Linked from "#lib/components/Linked.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { chatTime, formatDate } from "#lib/format.js";

  type Props = {
    message: Message;
    /** the first of a group: shows the author and the time */
    head: boolean;
    /** the message a link pointed at */
    highlight: boolean;
    onretry: () => void;
  };

  let { message: m, head, highlight, onretry }: Props = $props();

  let unfolded = $state(false);
  let shownHidden = $state(false);
  const long = $derived(m.text.length > LONG && !unfolded);
</script>

<div
  class={[
    "group/message border-l-2 px-3",
    head ? "pt-2" : "pt-0.5",
    m.unread ? "border-accent" : "border-transparent",
    m.mention && "bg-warning-soft/60",
    m.self && "bg-accent-soft/20",
    highlight && "ring-2 ring-accent ring-inset"
  ]}
  data-message={m.localId}>
  {#if head}
    <div class="flex items-baseline gap-2 text-sm">
      <Username name={m.author} />
      {#if m.self}<span class="text-xs text-accent">you</span>{/if}
      <time class="text-xs text-fg-subtle" datetime={m.timestamp} title={formatDate(m.timestamp)}
        >{chatTime(m.timestamp, clock.now)}</time>
      {#if m.replayed}
        <Tooltip text="Delivered late by the server">
          {#snippet child({ props })}
            <span {...props} class="text-xs text-fg-subtle italic">replayed</span>
          {/snippet}
        </Tooltip>
      {/if}
    </div>
  {/if}
  {#if m.hidden && !shownHidden}
    <button
      type="button"
      class="text-xs text-fg-muted italic hover:underline"
      onclick={() => (shownHidden = true)}>1 hidden message from a blocked user</button>
  {:else}
    <p class="text-sm wrap-anywhere whitespace-pre-wrap">
      <Linked text={long ? `${m.text.slice(0, LONG)}…` : m.text} />
      {#if long}
        <button
          type="button"
          class="text-xs text-accent hover:underline"
          onclick={() => (unfolded = true)}>show more</button>
      {/if}
    </p>
  {/if}
  {#if m.sending === "sending"}
    <span class="flex items-center gap-1 text-xs text-fg-subtle"
      ><ClockIcon class="size-3" />sending</span>
  {:else if m.sending === "failed"}
    <span class="flex items-center gap-1 text-xs text-danger">
      not sent
      <Button variant="link" class="text-xs" onclick={onretry}>retry</Button>
    </span>
  {/if}
</div>
