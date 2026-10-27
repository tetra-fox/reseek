<script lang="ts">
  import { goto } from "$app/navigation";
  import BanIcon from "@lucide/svelte/icons/ban";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import PinIcon from "@lucide/svelte/icons/pin";
  import PinOffIcon from "@lucide/svelte/icons/pin-off";
  import SearchIcon from "@lucide/svelte/icons/search";
  import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
  import UserIcon from "@lucide/svelte/icons/user";
  import { DropdownMenu, Popover } from "bits-ui";

  import { Busy } from "#lib/busy.svelte.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { interaction } from "#lib/interaction.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import { users } from "#lib/users.svelte.js";
  import { cn } from "#lib/utils.js";

  import { links, pin } from "./actions";
  import { blocking, unblock } from "./block.svelte";
  import GroupBadge from "./GroupBadge.svelte";
  import { CLOSE_DELAY_MS, LONG_PRESS_MS, OPEN_DELAY_MS, open } from "./hover.svelte";
  import UserCard from "./UserCard.svelte";

  type Props = {
    /** the username, shown exactly as it is */
    name: string;
    /** the folder the name was shown with, which "Search their files" starts from */
    folder?: string;
    class?: string;
  };

  let { name, folder, class: className }: Props = $props();

  const me = Symbol("username");
  let link = $state<HTMLAnchorElement | null>(null);
  let shown = $state<"card" | "menu" | null>(null);
  /** the card was opened from the keyboard, so focus goes into it and back to the link after */
  let keyboard = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pressed = false;

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

  const show = (what: "card" | "menu", byKeyboard = false) => {
    clearTimeout(timer);
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
  };

  const later = (run: () => void, ms: number) => {
    clearTimeout(timer);
    timer = setTimeout(run, ms);
  };

  const enter = (e: PointerEvent) => {
    // touch has no hover card: a tap follows the link and a long press opens the menu
    if (e.pointerType === "touch") return;
    if (active === "card") clearTimeout(timer);
    else if (!active) later(() => show("card"), OPEN_DELAY_MS);
  };

  const leave = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
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

<span
  class={cn(
    "group/username inline-flex max-w-full items-center gap-0.5 align-baseline",
    className
  )}>
  <a
    bind:this={link}
    href={userHref(name)}
    class="truncate font-medium text-fg underline-offset-2 hover:underline"
    onpointerenter={enter}
    onpointerleave={leave}
    onpointerdown={down}
    onpointerup={() => clearTimeout(timer)}
    onclick={(e) => {
      // the click that ends a long press only opened the menu
      if (pressed) e.preventDefault();
      pressed = false;
    }}
    oncontextmenu={(e) => {
      e.preventDefault();
      show("menu");
    }}
    onkeydown={keydown}>{name}</a>
  <button
    type="button"
    tabindex="-1"
    aria-label={`Menu for ${name}`}
    class="rounded-badge text-fg-muted opacity-0 transition-opacity group-focus-within/username:opacity-100 group-hover/username:opacity-100 hover:bg-hover"
    onclick={() => show("menu")}>
    <ChevronDownIcon class="size-3.5" />
  </button>
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
          e.pointerType !== "touch" && later(hide, CLOSE_DELAY_MS)}
        role="dialog"
        aria-label={`Profile of ${name}`}
        class="z-50 w-80 max-w-[calc(100vw-1rem)] rounded-card border bg-overlay p-3 text-fg shadow-raised outline-none motion-safe:animate-popover-in">
        <UserCard {name} {user} onblock={blockOrAsk} />
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
        <span class="truncate font-mono font-medium">{name}</span>
        {#if user}<span class="ml-auto"><GroupBadge group={user.group} /></span>{/if}
      </div>
      <Menu.Separator />
      <Menu.Item onSelect={() => goto(userHref(name))}><UserIcon />View profile</Menu.Item>
      <Menu.Item onSelect={() => goto(to.browse)}><FolderOpenIcon />Browse shares</Menu.Item>
      {#if readWrite}
        <Menu.Item onSelect={() => goto(to.message)}><MessageSquareIcon />Message</Menu.Item>
      {/if}
      <Menu.Item onSelect={() => goto(to.search)}><SearchIcon />Search their files</Menu.Item>
      {#if readWrite}
        <Menu.Separator />
        <Menu.Item onSelect={togglePin}>
          {#if user?.pinned}<PinOffIcon />Unpin{:else}<PinIcon />Pin{/if}
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
