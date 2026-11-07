<script lang="ts">
  import { afterNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import FlaskConicalIcon from "@lucide/svelte/icons/flask-conical";
  import SunMoonIcon from "@lucide/svelte/icons/sun-moon";
  import { Dialog } from "bits-ui";
  import { toggleMode } from "mode-watcher";
  import { tick } from "svelte";

  import { screenHref } from "#lib/analysis/slots.svelte.js";
  import { app } from "#lib/app.svelte.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Scrim from "#lib/components/ui/Scrim.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { href, navCurrent, navItems, type NavItem, route, systemItem } from "#lib/nav.js";
  import { sidebar } from "#lib/shell/sidebar.svelte.js";
  import { unseen } from "#lib/transfers/unseen.svelte.js";
  import { crossFade } from "#lib/ui/motion.js";

  import AccountMenu from "./AccountMenu.svelte";
  import ConnectButton from "./ConnectButton.svelte";
  import Logo from "./Logo.svelte";

  // a link in the phone sheet closes it
  afterNavigate(() => (sidebar.sheetOpen = false));

  const current = $derived(navCurrent(route(page.url).path)?.path);
  const canary = $derived(app.state?.version.isCanary ?? false);
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
          href={screenHref(item.path)}
          aria-label={sidebar.collapsed ? item.title : undefined}
          aria-current={current === item.path ? "page" : undefined}
          class="relative nav-item">
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
            <Tooltip
              text={`${fresh.succeeded} finished, ${fresh.failed} failed since last viewed`}
              side="right"
              dense>
              {#snippet child({ props: badge })}
                <Badge
                  {...badge}
                  tone={fresh.failed ? "danger" : "success"}
                  class={sidebar.collapsed
                    ? "absolute top-0.5 right-0.5 px-1 py-0 text-[0.625rem]"
                    : "ml-auto"}
                  aria-label={`${fresh.succeeded} finished, ${fresh.failed} failed`}
                  >{fresh.failed || fresh.succeeded}</Badge>
              {/snippet}
            </Tooltip>
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
        <span class="text-sm font-medium">reseek</span>
        <span class="text-[0.6875rem] text-fg-muted">Soulseek daemon</span>
      </span>
    </a>
    <nav aria-label="Screens" class="flex flex-1 flex-col justify-between gap-4 pt-4 pb-2">
      <ul class="flex flex-col gap-1">
        {#if canary}
          <li class="pointer-events-none nav-item">
            <FlaskConicalIcon class="size-4 shrink-0 text-warning" />
            <span class={["truncate", sidebar.collapsed && "hidden"]}>Canary</span>
          </li>
        {/if}
        {#each navItems as item (item.path)}
          {@render link(item)}
        {/each}
      </ul>
      <ul class="flex flex-col gap-1">
        <li>
          <Tooltip text={sidebar.collapsed ? "Theme" : undefined} side="right">
            {#snippet child({ props })}
              <button
                {...props}
                type="button"
                class="nav-item"
                aria-label="Switch between light and dark theme"
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
      "h-full shrink-0 transition-[width] duration-(--motion-fast) ease-(--ease-out-soft)",
      sidebar.collapsed ? "w-[calc(var(--spacing-control)+1.5rem+1px)]" : "w-60"
    ]}>
    {@render panel()}
  </div>
{/if}
