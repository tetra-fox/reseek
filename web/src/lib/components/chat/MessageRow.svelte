<script lang="ts">
  import ClockIcon from "@lucide/svelte/icons/clock";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import ReplyIcon from "@lucide/svelte/icons/reply";

  import { LONG, type Message } from "#lib/chat/messages.js";
  import { actionOf } from "#lib/chat/rich.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import { clock } from "#lib/clock.svelte.js";
  import { nameColour } from "#lib/colours.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Avatar from "#lib/components/users/Avatar.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { chatTime, formatDate, shortTime } from "#lib/format.js";
  import { chatHref } from "#lib/nav.js";
  import { users } from "#lib/users.svelte.js";

  import ChatText from "./ChatText.svelte";

  type Props = {
    message: Message;
    /** the first of a group: shows the face, the author and the time */
    head: boolean;
    /** the message a link pointed at */
    highlight: boolean;
    /** the words marked in the text: our name and the watched words */
    watch: string[];
    /** a room's message, which offers a private chat with its author */
    room: boolean;
    /** quotes the message in the composer, when one can be sent */
    onreply?: (message: Message) => void;
    onretry: () => void;
  };

  let { message: m, head, highlight, watch, room, onreply, onretry }: Props = $props();

  let unfolded = $state(false);
  let shownHidden = $state(false);
  const long = $derived(m.text.length > LONG && !unfolded);
  const action = $derived(actionOf(m.text));
  const text = $derived(action ?? m.text);
</script>

{#snippet body()}
  <!-- our own messages mark nothing: our name in them is no mention -->
  <ChatText text={long ? `${text.slice(0, LONG)}…` : text} watch={m.self ? [] : watch} />
  {#if long}
    <button
      type="button"
      class="ml-1 text-xs text-accent hover:underline"
      onclick={() => (unfolded = true)}>show more</button>
  {/if}
{/snippet}

<div
  class={[
    "group/message relative grid grid-cols-[2.25rem_minmax(0,1fr)] gap-x-2.5 border-l-2 pr-3 pl-2.5",
    head ? "pt-2.5 pb-0.5" : "py-px",
    m.mention
      ? "border-warning bg-warning-soft/45"
      : m.unread
        ? "border-accent"
        : "border-transparent",
    !m.mention && "hover:bg-hover/60",
    highlight && "bg-accent-soft/50 ring-1 ring-accent/50 ring-inset"
  ]}
  data-message={m.localId}>
  <div class="flex justify-end">
    {#if head}
      <Avatar
        name={m.author}
        pictureUrl={users.get(m.author)?.info?.pictureUrl}
        class="mt-0.5 size-9 text-sm" />
    {:else}
      <time
        class="invisible pt-px text-[0.625rem] leading-5 whitespace-nowrap text-fg-subtle tabular-nums group-hover/message:visible"
        datetime={m.timestamp}
        title={formatDate(m.timestamp)}>{shortTime(m.timestamp)}</time>
    {/if}
  </div>
  <div class="min-w-0">
    {#if head}
      <div class="flex items-baseline gap-2 leading-5">
        <Username name={m.author} card class="text-sm font-semibold" />
        {#if m.self}<span class="text-xs text-fg-subtle">you</span>{/if}
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
    {:else if action}
      <!-- an action reads as a sentence: the name starts it unless the header above shows it -->
      <p class="text-sm leading-5 wrap-anywhere whitespace-pre-wrap text-fg-muted italic">
        {#if !head}<span class="font-medium not-italic" style:color={nameColour(m.author)}
            >{m.author}</span
          >{" "}{/if}{@render body()}
      </p>
    {:else}
      <p class="text-sm leading-5 wrap-anywhere whitespace-pre-wrap">{@render body()}</p>
    {/if}
    {#if m.sending === "sending"}
      <span class="flex items-center gap-1 text-xs text-fg-subtle"
        ><ClockIcon class="size-3" />sending</span>
    {:else if m.sending === "failed"}
      <span class="flex items-center gap-1 text-xs text-danger">
        Not sent
        <Button variant="link" class="text-xs" onclick={onretry}>retry</Button>
      </span>
    {/if}
  </div>
  {#if !m.hidden && m.sending === undefined}
    <!-- what can be done with the message, shown over its top right corner on hover -->
    <div
      class="absolute -top-3 right-3 z-10 hidden items-center rounded-control border bg-overlay p-0.5 shadow-raised group-focus-within/message:flex group-hover/message:flex">
      {#if onreply}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Reply with a quote"
          title="Reply with a quote"
          onclick={() => onreply(m)}><ReplyIcon /></Button>
      {/if}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Copy the text"
        title="Copy the text"
        onclick={() => void copyText(m.text)}><CopyIcon /></Button>
      {#if room && !m.self}
        <Button
          variant="ghost"
          size="icon-sm"
          href={chatHref(m.author)}
          aria-label={`Message ${m.author}`}
          title={`Message ${m.author}`}><MessageSquareIcon /></Button>
      {/if}
    </div>
  {/if}
</div>
