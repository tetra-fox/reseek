<script lang="ts">
  import { goto } from "$app/navigation";
  import BanIcon from "@lucide/svelte/icons/ban";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import SearchIcon from "@lucide/svelte/icons/search";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserIcon from "@lucide/svelte/icons/user";
  import UserCheckIcon from "@lucide/svelte/icons/user-check";
  import UserPlusIcon from "@lucide/svelte/icons/user-plus";
  import { DropdownMenu, Popover } from "bits-ui";

  import { Busy } from "#lib/busy.svelte.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import { nameColour } from "#lib/colours.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { interaction } from "#lib/interaction.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { users } from "#lib/users.svelte.js";
  import { cn } from "#lib/utils.js";
  import { BUDDY } from "#lib/wording.js";

  import { links, pin } from "./actions";
  import { blocking, unblock } from "./block.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import { CLOSE_DELAY_MS, LONG_PRESS_MS, OPEN_DELAY_MS, open } from "./hover.svelte";
  import UserCard from "./UserCard.svelte";

  type Props = {
    /** the username, shown exactly as it is */
    name: string;
    /** the folder the name was shown with, which "Search their shares" starts from */
    folder?: string;
    /**
     * a click opens the card and keeps it open, and a double click opens a private chat, as
     * names in a chat do; elsewhere a click goes to the profile
     */
    card?: boolean;
    class?: string;
  };

  let { name, folder, card = false, class: className }: Props = $props();

  const me = Symbol("username");
  let link = $state<HTMLAnchorElement | null>(null);
  let shown = $state<"card" | "menu" | null>(null);
  /** the card was opened from the keyboard, so focus goes into it and back to the link after */
  let keyboard = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pressed = false;
  /** the card was opened by a click, which keeps it open when the pointer leaves */
  let clicked = false;

  const user = $derived(users.get(name));
  const to = $derived(links(name, folder));
  const readWrite = $derived(session.can("ReadWrite"));

  // another username's card or menu opening closes this one
  const active = $derived(open.owner === me ? shown : null);
  // the list the name is in holds still while its card or menu is open
  $effect(() => (active ? interaction.hold() : undefined));
  // a scroll of what is under the card closes it
  $effect(() => {
    if (active !== "card") return;
    const close = () => hide();
    window.addEventListener("scroll", close, { capture: true, passive: true });
    return () => window.removeEventListener("scroll", close, { capture: true });
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
    if (e.key === " " || (e.key === "ArrowDown" && e.altKey)) {
      e.preventDefault();
      show("card", true);
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      show("menu", true);
    }
  };

  const busy = new Busy();
  const togglePin = () =>
    busy.run(async () => {
      const outcome = await pin(name, !user?.pinned);
      if (outcome.ok) await users.load(name, { fresh: true });
      return outcome;
    });
  /** Blocking opens the block dialog; unblocking is one click, with an undo */
  const blockOrAsk = () => {
    shown = null;
    if (user?.block) void unblock(name);
    else blocking.ask(name);
  };
</script>

<!-- a name takes only its text's width: its menu opens from a right-click, a long press,
     Shift+F10 or the card's More button, never from a control beside it -->
<span class={cn("inline-flex max-w-full items-center align-baseline", className)}>
  <a
    bind:this={link}
    href={userHref(name)}
    class="truncate font-medium underline-offset-2 hover:underline"
    style:color={nameColour(name)}
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
    onkeydown={keydown}>{name}</a>
</span>

{#if active === "card"}
  <Popover.Root open onOpenChange={(o) => !o && hide()}>
    <Popover.Portal>
      <Popover.Content
        customAnchor={link}
        side="bottom"
        align="start"
        sideOffset={4}
        collisionPadding={8}
        trapFocus={keyboard}
        onOpenAutoFocus={(e) => !keyboard && e.preventDefault()}
        onCloseAutoFocus={(e) => e.preventDefault()}
        onpointerenter={() => clearTimeout(timer)}
        onpointerleave={(e: PointerEvent) =>
          e.pointerType !== "touch" && !clicked && later(hide, CLOSE_DELAY_MS)}
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
        <span class="truncate font-mono font-medium" style:color={nameColour(name)}>{name}</span>
        {#if user}<span class="ml-auto"><GroupBadge group={user.group} /></span>{/if}
      </div>
      <Menu.Separator />
      <Menu.Item onSelect={() => goto(userHref(name))}><UserIcon />View profile</Menu.Item>
      <Menu.Item onSelect={() => goto(to.browse)}><FolderOpenIcon />Browse shares</Menu.Item>
      {#if readWrite}
        <Menu.Item onSelect={() => goto(to.message)}><MessageSquareIcon />Message</Menu.Item>
      {/if}
      <Menu.Item onSelect={() => goto(to.search)}><SearchIcon />Search their shares</Menu.Item>
      {#if readWrite}
        <Menu.Separator />
        <Menu.Item onSelect={togglePin}>
          {#if user?.pinned}<UserCheckIcon />{BUDDY.remove}{:else}<UserPlusIcon />{BUDDY.add}{/if}
        </Menu.Item>
        <Menu.Item onSelect={blockOrAsk}>
          {#if user?.block}<ShieldCheckIcon />Unblock{:else}<BanIcon />Block{/if}
        </Menu.Item>
      {/if}
      <Menu.Separator />
      <Menu.Item onSelect={() => void copyText(name)}><CopyIcon />Copy username</Menu.Item>
    </Menu.Content>
  </DropdownMenu.Root>
{/if}
