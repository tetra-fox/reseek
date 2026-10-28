<script lang="ts">
  import { afterNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import FlaskConicalIcon from "@lucide/svelte/icons/flask-conical";
  import SunMoonIcon from "@lucide/svelte/icons/sun-moon";
  import UserSearchIcon from "@lucide/svelte/icons/user-search";
  import { Dialog } from "bits-ui";
  import { toggleMode } from "mode-watcher";
  import { tick } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Scrim from "#lib/components/ui/Scrim.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { crossFade } from "#lib/motion.js";
  import { href, navCurrent, navItems, type NavItem, route, systemItem } from "#lib/nav.js";
  import { unseen } from "#lib/transfers/unseen.svelte.js";

  import AccountMenu from "./AccountMenu.svelte";
  import ConnectButton from "./ConnectButton.svelte";
  import Logo from "./Logo.svelte";
  import { sidebar } from "./sidebar.svelte";

  // a link in the phone sheet closes it
  afterNavigate(() => (sidebar.sheetOpen = false));

  const current = $derived(navCurrent(route(page.url).path)?.path);
  const agent = $derived(app.mode === "agent");
  const canary = $derived(app.state?.version.isCanary ?? false);

  const itemClass =
    "flex h-control w-full items-center gap-2.5 rounded-control px-2 text-sm whitespace-nowrap text-fg-muted transition-colors duration-(--motion-fast) select-none hover:bg-hover hover:text-fg active:bg-press aria-[current]:nav-current";
</script>

{#snippet link(item: NavItem)}
  {@const direction =
    item.path === "/downloads" ? "download" : item.path === "/uploads" ? "upload" : null}
  {@const fresh = direction && current !== item.path ? unseen.of(direction) : null}
  <li>
    <Tooltip text={sidebar.collapsed ? item.title : undefined} side="right">
      {#snippet child({ props })}
        <a
          {...props}
          href={href(item.path)}
          aria-label={sidebar.collapsed ? item.title : undefined}
          aria-current={current === item.path ? "page" : undefined}
          class={[itemClass, "relative"]}>
          <span class="relative shrink-0">
            <item.icon class={["size-4", current === item.path && "text-accent"]} />
            {#if item.path === "/chat" && chat.total && sidebar.collapsed}
              <span
                class={[
                  "absolute -top-1 -right-1 size-2 rounded-full",
                  chat.totalMentions ? "bg-accent" : "bg-danger"
                ]}></span>
            {/if}
          </span>
          <span class={["truncate", sidebar.collapsed && "hidden"]}>{item.title}</span>
          {#if fresh && (fresh.failed || fresh.succeeded)}
            <Badge
              tone={fresh.failed ? "danger" : "success"}
              class={sidebar.collapsed
                ? "absolute top-0.5 right-0.5 px-1 py-0 text-[0.625rem]"
                : "ml-auto"}
              title={`${fresh.succeeded} finished, ${fresh.failed} failed since you last looked`}
              aria-label={`${fresh.succeeded} finished, ${fresh.failed} failed`}
              >{fresh.failed || fresh.succeeded}</Badge>
          {/if}
          {#if item.path === "/chat" && chat.total && !sidebar.collapsed}
            <Badge
              tone={chat.totalMentions ? "accent" : "danger"}
              class="ml-auto"
              aria-label={`${chat.total} unread`}>{chat.total}</Badge>
          {/if}
        </a>
      {/snippet}
    </Tooltip>
  </li>
{/snippet}

{#snippet panel()}
  <div
    class={[
      "flex h-full flex-col gap-2 overflow-x-hidden overflow-y-auto bg-surface p-3",
      sidebar.phone ? "rounded-card border shadow-raised" : "border-r"
    ]}>
    <a href={href("/dashboard")} class="flex h-control items-center gap-2.5" aria-label="reseek">
      <Logo />
      <span class={["flex flex-col leading-none", sidebar.collapsed && "hidden"]}>
        <span class="text-sm font-semibold tracking-tight">reseek</span>
        <span class="text-[0.6875rem] text-fg-muted">Soulseek daemon</span>
      </span>
    </a>
    <nav aria-label="Screens" class="flex flex-1 flex-col justify-between gap-4 pt-4 pb-2">
      <ul class="flex flex-col gap-1">
        {#if canary}
          <li class={[itemClass, "pointer-events-none"]}>
            <FlaskConicalIcon class="size-4 shrink-0 text-warning" />
            <span class={["truncate", sidebar.collapsed && "hidden"]}>Canary</span>
          </li>
        {/if}
        {#if agent}
          <!-- a relay agent has only the System area, so the screens are one plain label -->
          <li class={[itemClass, "pointer-events-none"]}>
            <UserSearchIcon class="size-4 shrink-0" />
            <span class={["truncate", sidebar.collapsed && "hidden"]}>Agent Mode</span>
          </li>
        {:else}
          {#each navItems as item (item.path)}
            {@render link(item)}
          {/each}
        {/if}
      </ul>
      <ul class="flex flex-col gap-1">
        <li>
          <Tooltip text={sidebar.collapsed ? "Theme" : undefined} side="right">
            {#snippet child({ props })}
              <button
                {...props}
                type="button"
                class={itemClass}
                aria-label="Toggle dark and light theme"
                onclick={() =>
                  void crossFade(async () => {
                    toggleMode();
                    await tick();
                  })}>
                <SunMoonIcon class="size-4 shrink-0" />
                <span class={["truncate", sidebar.collapsed && "hidden"]}>Theme</span>
              </button>
            {/snippet}
          </Tooltip>
        </li>
        <li><ConnectButton collapsed={sidebar.collapsed} /></li>
        {@render link(systemItem)}
      </ul>
    </nav>
    <AccountMenu />
  </div>
{/snippet}

{#if sidebar.phone}
  <Dialog.Root bind:open={sidebar.sheetOpen}>
    <Dialog.Portal>
      <Scrim />
      <Dialog.Content
        class="fixed inset-y-0 left-0 z-50 w-72 p-2 data-[state=closed]:animate-sheet-out data-[state=open]:animate-sheet-in">
        <Dialog.Title class="sr-only">Navigation</Dialog.Title>
        {@render panel()}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{:else}
  <!-- the icon strip is one control wide plus the panel's padding and its border -->
  <div
    class={[
      "sticky top-0 h-[calc(100svh-2.5rem)] shrink-0 transition-[width] duration-(--motion-fast) ease-(--ease-out-soft)",
      sidebar.collapsed ? "w-[calc(var(--spacing-control)+1.5rem+1px)]" : "w-60"
    ]}>
    {@render panel()}
  </div>
{/if}
