<script lang="ts">
  import { goto } from "$app/navigation";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import { DropdownMenu, Popover } from "bits-ui";
  import type { HTMLAttributes } from "svelte/elements";

  import { SERVER } from "#lib/chat/notices.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { interaction } from "#lib/lists/interaction.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { USERNAMES } from "#lib/ui/bindings.js";
  import { cn } from "#lib/ui/cn.js";
  import { isPressed } from "#lib/ui/keys.js";
  import { safeArea } from "#lib/ui/safearea.svelte.js";
  import { canManage, links } from "#lib/users/actions.js";
  import { toggleBlock } from "#lib/users/block.svelte.js";
  import { nameColor } from "#lib/users/colors.js";
  import { queueOf } from "#lib/users/groups.js";
  import { CLOSE_DELAY_MS, LONG_PRESS_MS, OPEN_DELAY_MS, open } from "#lib/users/hover.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import GroupBadge from "./GroupBadge.svelte";
  import NameText from "./NameText.svelte";
  import UserCard from "./UserCard.svelte";
  import UserManageItems from "./UserManageItems.svelte";
  import UserMenuItems from "./UserMenuItems.svelte";

  type Props = {
    /** the username, shown exactly as it is */
    name: string;
    /** the folder the name was shown with, which "Search shares" starts from */
    folder?: string;
    /**
     * a click opens the card and keeps it open, and a double click opens a private chat, as
     * names in a chat do; elsewhere a click goes to the profile
     */
    card?: boolean;
    /**
     * the name in its look with no link, card or menu, where a link can't go: an option a click
     * picks, a tooltip
     */
    plain?: boolean;
    class?: string;
  };

  let { name, folder, card = false, plain = false, class: className }: Props = $props();

  const me = Symbol("username");
  let link = $state<HTMLAnchorElement | null>(null);
  let cardBox = $state<HTMLElement | null>(null);
  let shown = $state<"card" | "menu" | null>(null);
  /** the card was opened from the keyboard, so focus goes into it and back to the link after */
  let keyboard = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pressed = false;
  /** the card was opened by a click, which keeps it open when the pointer leaves */
  let clicked = false;

  const user = $derived(users.get(name));
  const queue = $derived(user ? queueOf(user) : null);
  const to = $derived(links(name, folder));
  const readWrite = $derived(session.can("ReadWrite"));

  // another username's card or menu opening closes this one
  const active = $derived(open.owner === me ? shown : null);
  // the list the name is in holds still while its card or menu is open
  $effect(() => (active ? interaction.hold() : undefined));
  // the user scrolling the page under the card closes it. A list that moves itself, to keep
  // its view as lines arrive, does not: the card follows its name, and hides while the name is
  // out of view
  $effect(() => {
    if (active !== "card") return;
    const close = (e: Event) => {
      if (!(e.target instanceof Node && cardBox?.contains(e.target))) hide();
    };
    const options = { capture: true, passive: true };
    window.addEventListener("wheel", close, options);
    window.addEventListener("touchmove", close, options);
    return () => {
      window.removeEventListener("wheel", close, options);
      window.removeEventListener("touchmove", close, options);
    };
  });

  const show = (what: "card" | "menu", byKeyboard = false, byClick = false) => {
    clearTimeout(timer);
    clicked = byClick;
    keyboard = byKeyboard;
    open.owner = me;
    shown = what;
    void users.load(name);
  };

  const hide = () => {
    clearTimeout(timer);
    if (keyboard && shown === "card") link?.focus();
    shown = null;
    keyboard = false;
    clicked = false;
  };

  const later = (run: () => void, ms: number) => {
    clearTimeout(timer);
    timer = setTimeout(run, ms);
  };

  const enter = (e: PointerEvent) => {
    // touch has no hover card: a tap is a click, which opens the card on a name with `card` and
    // follows the link elsewhere, and a long press opens the menu
    if (e.pointerType === "touch") return;
    if (active === "card") clearTimeout(timer);
    else if (!active) later(() => show("card"), OPEN_DELAY_MS);
  };

  const leave = (e: PointerEvent) => {
    if (e.pointerType === "touch" || clicked) return;
    if (active === "card") later(hide, CLOSE_DELAY_MS);
    else if (!active) clearTimeout(timer);
  };

  const down = (e: PointerEvent) => {
    pressed = false;
    if (e.pointerType !== "touch") return;
    later(() => {
      pressed = true;
      show("menu");
    }, LONG_PRESS_MS);
  };

  const keydown = (e: KeyboardEvent) => {
    if (isPressed(USERNAMES.card, e)) {
      e.preventDefault();
      show("card", true);
    } else if (isPressed(USERNAMES.menu, e)) {
      e.preventDefault();
      show("menu", true);
    }
  };

  /** the box a name sits in, no wider than its text or than the room it has */
  const box = "inline-flex max-w-full min-w-0 items-center align-baseline";
  /** the name's own look, as a link or as text; the server's name has no color of a user's */
  const look: HTMLAttributes<HTMLElement> = $derived({
    dir: "auto",
    class: "truncate font-medium",
    style: name === SERVER ? undefined : `color: ${nameColor(name)}`
  });

  const blockOrAsk = () => {
    shown = null;
    toggleBlock(name, !!user?.block);
  };
</script>

<!-- a name as text: a plain one, or where the name the server's notices come from turns up, as
     in the events, which is no user. Written with nothing between its tags, so text around it, as
     in a tooltip, reads on from the name without a space -->
{#if plain || name === SERVER}
  <span class={cn(box, className)}><span {...look}><NameText {name} /></span></span>
{:else}
  <!-- a name takes only its text's width: its menu opens from a right-click, a long press,
       Shift+F10 or the card's More button, never from a control beside it -->
  <span class={cn(box, className)}>
    <a
      bind:this={link}
      href={userHref(name)}
      {...look}
      aria-label={name.trim() === name ? undefined : name}
      class={[look.class, "underline-offset-2 hover:underline"]}
      onpointerenter={enter}
      onpointerleave={leave}
      onpointerdown={down}
      onpointerup={() => clearTimeout(timer)}
      onclick={(e) => {
        // the click that ends a long press only opened the menu
        if (pressed) e.preventDefault();
        pressed = false;
        // a modified click still opens the profile where the browser puts it
        if (!card || e.ctrlKey || e.metaKey || e.shiftKey || e.button !== 0) return;
        e.preventDefault();
        if (active === "card" && clicked) hide();
        else show("card", false, true);
      }}
      ondblclick={(e) => {
        if (!card || !readWrite) return;
        e.preventDefault();
        hide();
        void goto(to.message);
      }}
      oncontextmenu={(e) => {
        e.preventDefault();
        show("menu");
      }}
      onkeydown={keydown}><NameText {name} /></a>

    <!-- the card and the menu render elsewhere through portals; kept inside the flex box, whose
       whitespace between children is not drawn, the name has no space after it -->
    {#if active === "card"}
      <Popover.Root open onOpenChange={(o) => !o && hide()}>
        <Popover.Portal>
          <Popover.Content
            bind:ref={cardBox}
            customAnchor={link}
            hideWhenDetached
            side="bottom"
            align="start"
            sideOffset={4}
            collisionPadding={safeArea.padding(8)}
            trapFocus={keyboard}
            onOpenAutoFocus={(e) => !keyboard && e.preventDefault()}
            onCloseAutoFocus={(e) => e.preventDefault()}
            onpointerenter={() => clearTimeout(timer)}
            onpointerleave={(e: PointerEvent) =>
              e.pointerType !== "touch" && !clicked && later(hide, CLOSE_DELAY_MS)}
            onclick={(e: MouseEvent) => {
              // following a link in the card closes it, opening one in another tab does not
              const followed = e.target instanceof Element && e.target.closest("a[href]");
              if (followed && !e.ctrlKey && !e.metaKey && !e.shiftKey) hide();
            }}
            role="dialog"
            aria-label={`Profile of ${name}`}
            class="z-50 w-80 max-w-[calc(100vw-1rem)] rounded-card border bg-overlay p-3 text-fg shadow-raised outline-none motion-safe:animate-popover-in">
            <UserCard {name} {user} onblock={blockOrAsk} onmenu={() => show("menu", keyboard)} />
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    {/if}

    {#if active === "menu"}
      <DropdownMenu.Root open onOpenChange={(o) => !o && hide()}>
        <Menu.Content customAnchor={link} align="start" class="min-w-52">
          <div class="flex items-center gap-2 px-2 py-1.5 text-xs">
            <span
              class={[
                "size-2 rounded-full",
                user?.presence === "Online"
                  ? "bg-success"
                  : user?.presence === "Away"
                    ? "bg-warning"
                    : "bg-fg-subtle/50"
              ]}></span>
            <span class="truncate font-mono font-medium" style:color={nameColor(name)}>{name}</span>
            {#if queue}<span class="ml-auto"><GroupBadge group={queue} queue /></span>{/if}
          </div>
          <Menu.Separator />
          <UserMenuItems {name} {folder} />
          {#if canManage(name)}
            <Menu.Separator />
            <UserManageItems {name} />
          {/if}
          <Menu.Separator />
          <Menu.Item onSelect={() => void copyText(name)}><CopyIcon />Copy username</Menu.Item>
        </Menu.Content>
      </DropdownMenu.Root>
    {/if}
  </span>
{/if}
