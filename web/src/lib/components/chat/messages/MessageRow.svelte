<script lang="ts">
  import { goto } from "$app/navigation";
  import ClockIcon from "@lucide/svelte/icons/clock";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import ReplyIcon from "@lucide/svelte/icons/reply";
  import type { Component } from "svelte";

  import { actionable, type Message } from "#lib/chat/messages/messages.js";
  import { actionOf } from "#lib/chat/messages/rich.js";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Avatar from "#lib/components/users/Avatar.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { chatTime, formatDate, shortTime } from "#lib/format.js";
  import type { MenuAnchor } from "#lib/lists/linemenu.svelte.js";
  import { wakeOnPointer } from "#lib/lists/wake.js";
  import { MESSAGES } from "#lib/ui/bindings.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { isPressed, keyStep } from "#lib/ui/keys.js";
  import { reach } from "#lib/users/actions.js";
  import { pictureOf } from "#lib/users/profile.svelte.js";

  import LongText from "./LongText.svelte";

  type Props = {
    /** its line's key, which its menu finds it by */
    "line": string;
    "message": Message;
    /** the first of a group: shows the face, the author and the time */
    "head": boolean;
    /** the message a link pointed at */
    "highlight": boolean;
    /** the words marked in the text: our name and the watched words */
    "watch": string[];
    /** a room's message, which offers a private chat with its author */
    "room": boolean;
    /** a notice from the server: the time and the text, with nothing that belongs to a person */
    "notice"?: boolean;
    /** the keys are on it: it takes focus, and Tab goes from it to its actions */
    "active": boolean;
    /** what the list's line menu marks it with, which says whether its menu is open */
    "data-menu-open"?: true;
    /** where the list's open menu hangs: at the pointer, or from the message itself for the keys */
    "menu": MenuAnchor | null;
    /** closes its menu */
    "onclosemenu": () => void;
    /** quotes the message in the composer, when one can be sent */
    "onreply"?: (message: Message) => void;
    "onretry": () => void;
  };

  let {
    line,
    "message": m,
    head,
    highlight,
    watch,
    room,
    notice = false,
    active,
    "data-menu-open": menuOpen,
    menu,
    onclosemenu,
    onreply,
    onretry
  }: Props = $props();

  let shownHidden = $state(false);
  /** a mouse or a pen has come to the row, which then mounts its tooltips */
  let awake = $state(false);
  const pointed = wakeOnPointer(() => (awake = true));
  const action = $derived(notice ? null : actionOf(m.text));
  const text = $derived(action ?? m.text);

  /** One thing to do with the message: a function to run or a page to go to */
  type Action = { label: string; icon: Component } & (
    | { run: () => void; href?: never }
    | { href: string; run?: never }
  );

  /** What can be done with the message, on its toolbar and in its menu */
  const actions = $derived.by((): Action[] => {
    if (!actionable(m)) return [];
    return [
      ...(onreply ? [{ label: "Quote", icon: ReplyIcon, run: () => onreply(m) }] : []),
      { label: "Copy text", icon: CopyIcon, run: () => void copyText(m.text) },
      ...(room && !m.self
        ? reach(m.author, { only: ["message"] }).map((reached) => ({
            label: reached.of(m.author),
            icon: reached.icon,
            href: reached.href
          }))
        : [])
    ];
  });

  let row = $state<HTMLElement | null>(null);
  let toolbar = $state<HTMLElement | null>(null);
  /**
   * Tab from the message the keys are on goes to its actions, which come after its text;
   * Shift+Tab from the first of them, or Escape from any, comes back
   */
  const keydown = (e: KeyboardEvent) => {
    if (!active || !row || !toolbar) return;
    const first = toolbar.querySelector<HTMLElement>("a, button");
    const back =
      (keyStep(MESSAGES.actions, e) === -1 && e.target === first) ||
      (isPressed(MESSAGES.back, e) && toolbar.contains(e.target as Node));
    if (keyStep(MESSAGES.actions, e) === 1 && e.target === row && first) {
      e.preventDefault();
      first.focus();
    } else if (back) {
      e.preventDefault();
      row.focus();
    }
  };
</script>

{#snippet body()}
  <!-- our own messages and the server's mark nothing: our name in them is no mention -->
  <LongText {text} watch={m.self || notice ? [] : watch} />
{/snippet}

<!-- the tabindex is only ever -1: the message the keys are on takes focus from them, not Tab.
     Its keys are Tab and Escape between it and its actions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  bind:this={row}
  role="article"
  onkeydown={keydown}
  onpointerenter={pointed}
  class={[
    "group/message relative grid gap-x-2.5 border-l-2 pr-3 pl-2.5",
    notice ? "grid-cols-[4rem_minmax(0,1fr)]" : "grid-cols-[2.25rem_minmax(0,1fr)]",
    notice ? "py-1" : head ? "pt-2.5 pb-0.5" : "py-px",
    m.mention
      ? "border-warning bg-warning-soft/45"
      : m.unread
        ? "border-accent"
        : "border-transparent",
    !m.mention && "hover:bg-hover/60",
    highlight && "bg-accent-soft/50 ring-1 ring-accent/50 ring-inset",
    active &&
      "outline-none focus-within:bg-hover/60 focus-within:ring-2 focus-within:ring-accent focus-within:ring-inset",
    "data-menu-open:bg-hover/60 data-menu-open:ring-2 data-menu-open:ring-accent data-menu-open:ring-inset"
  ]}
  tabindex={active ? -1 : undefined}
  data-line={line}
  data-menu-open={menuOpen}
  data-message={m.localId}>
  <div class="flex justify-end">
    {#if head}
      <Avatar name={m.author} pictureUrl={pictureOf(m.author)} class="mt-0.5 size-9 text-sm" />
    {:else}
      <Tooltip text={formatDate(m.timestamp)} dense dormant={!awake}>
        {#snippet child({ props })}
          <time
            {...props}
            class={[
              "pt-px leading-5 whitespace-nowrap text-fg-muted tabular-nums",
              notice
                ? "text-xs"
                : "invisible text-[0.625rem] group-focus-within/message:visible group-hover/message:visible"
            ]}
            datetime={m.timestamp}>{shortTime(m.timestamp)}</time>
        {/snippet}
      </Tooltip>
    {/if}
  </div>
  <div class="min-w-0">
    {#if head}
      <div class="flex items-baseline gap-2 leading-5">
        <Username name={m.author} card class="text-sm font-semibold" />
        {#if m.self}<span class="shrink-0 text-xs text-fg-muted">you</span>{/if}
        <Tooltip text={formatDate(m.timestamp)} dense dormant={!awake}>
          {#snippet child({ props })}
            <time
              {...props}
              class="shrink-0 text-xs whitespace-nowrap text-fg-muted"
              datetime={m.timestamp}>{chatTime(m.timestamp, clock.now)}</time>
          {/snippet}
        </Tooltip>
        {#if m.replayed}
          <Tooltip text="Held by the server until you connected" dense dormant={!awake}>
            {#snippet child({ props })}
              <span {...props} class="min-w-0 truncate text-xs text-fg-muted italic"
                >sent while offline</span>
            {/snippet}
          </Tooltip>
        {/if}
        {#if m.automatic}
          <Tooltip text="Your away reply" dense dormant={!awake}>
            {#snippet child({ props })}
              <span {...props} class="min-w-0 truncate text-xs text-fg-muted italic"
                >sent automatically</span>
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
      <p
        class="cursor-text text-sm leading-5 wrap-anywhere whitespace-pre-wrap text-fg-muted italic"
        dir="auto">
        {#if !head}<Username name={m.author} card class="not-italic" />{" "}{/if}{@render body()}
      </p>
    {:else}
      <p class="cursor-text text-sm leading-5 wrap-anywhere whitespace-pre-wrap" dir="auto">
        {@render body()}
      </p>
    {/if}
    {#if m.sending === "sending"}
      <span class="flex items-center gap-1 text-xs text-fg-muted"
        ><ClockIcon class="size-3" />Sending</span>
    {:else if m.sending === "failed"}
      <span class="flex items-center gap-1 text-xs text-danger">
        Not sent
        <Button variant="link" class="text-xs" onclick={onretry}>Retry</Button>
      </span>
    {/if}
  </div>
  {#if actions.length}
    <!-- over the message's top right corner on hover, while focus is in it or its menu is open.
         Last in the message, so a screen reader reads the author and the text first. Every
         action is also in the menu, which a touch reaches with a long press -->
    <div
      bind:this={toolbar}
      role="group"
      aria-label="Message actions"
      class="absolute -top-3 right-3 z-10 hidden items-center rounded-control border bg-overlay p-0.5 shadow-raised group-focus-within/message:flex group-hover/message:flex group-data-menu-open/message:flex">
      {#each actions as action (action.label)}
        <Tooltip text={action.label} dormant={!awake}>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              href={action.href}
              aria-label={action.label}
              onclick={action.run}><action.icon /></Button>
          {/snippet}
        </Tooltip>
      {/each}
    </div>
  {/if}
  {#if menuOpen && menu}
    <Menu.Root open onOpenChange={(open) => !open && onclosemenu()}>
      <Menu.Content align={menu === row ? "end" : "start"} customAnchor={menu}>
        {#each actions as action (action.label)}
          <Menu.Item onSelect={action.run ?? (() => void goto(action.href))}>
            <action.icon />{action.label}
          </Menu.Item>
        {/each}
      </Menu.Content>
    </Menu.Root>
  {/if}
</div>
