<script lang="ts">
  import { afterNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import FlaskConicalIcon from "@lucide/svelte/icons/flask-conical";
  import Rows3Icon from "@lucide/svelte/icons/rows-3";
  import Rows4Icon from "@lucide/svelte/icons/rows-4";
  import SunMoonIcon from "@lucide/svelte/icons/sun-moon";
  import { Dialog } from "bits-ui";
  import { toggleMode } from "mode-watcher";
  import { tick } from "svelte";
  import { scale } from "svelte/transition";

  import { screenHref } from "#lib/analysis/slots.svelte.js";
  import { app } from "#lib/app.svelte.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Scrim from "#lib/components/ui/Scrim.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { DENSITY_LABELS } from "#lib/lists/density.js";
  import { href, navCurrent, navItems, type NavItem, route, systemItem } from "#lib/nav.js";
  import { settings } from "#lib/settings.svelte.js";
  import { sidebar } from "#lib/shell/sidebar.svelte.js";
  import { unseen } from "#lib/transfers/unseen.svelte.js";
  import { crossFade, easeOutSoft, motionFast } from "#lib/ui/motion.js";

  import AccountMenu from "./AccountMenu.svelte";
  import ConnectButton from "./ConnectButton.svelte";
  import Logo from "./Logo.svelte";

  // a link in the phone sheet closes it
  afterNavigate(() => (sidebar.sheetOpen = false));

  const current = $derived(navCurrent(route(page.url).path)?.path);
  const canary = $derived(app.state?.version.isCanary ?? false);
  const density = $derived(`Density: ${DENSITY_LABELS[settings.density]}`);
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
          <!-- kept while collapsed, so it pops in as the count leaves 0 and not as the sidebar
               opens -->
          {#if item.path === "/chat" && chat.total}
            <span
              class={["ml-auto flex", sidebar.collapsed && "hidden"]}
              in:scale={{ start: 0.9, duration: motionFast(), easing: easeOutSoft }}>
              <Badge
                tone={chat.totalMentions ? "accent" : "danger"}
                aria-label={`${chat.total} unread`}>{chat.total}</Badge>
            </span>
          {/if}
        </a>
      {/snippet}
    </Tooltip>
  </li>
{/snippet}

{#snippet panel()}
  <div
    class={[
      "flex h-full flex-col gap-2 overflow-x-hidden overflow-y-auto px-3 pb-3",
      sidebar.phone ? "rounded-card border bg-surface pt-3 shadow-raised" : "bg-page pt-2"
    ]}>
    <!-- beside the screen's panel, the name takes the panel's top inset and border and its
         header's height, so it sits level with the screen's title -->
    <a
      href={href("/dashboard")}
      class={["flex items-center gap-2.5", sidebar.phone ? "h-control" : "mt-px h-header"]}
      aria-label="reseek">
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
        <li>
          <Tooltip text={sidebar.collapsed ? density : undefined} side="right">
            {#snippet child({ props })}
              <button
                {...props}
                type="button"
                class="nav-item"
                aria-label={density}
                onclick={() =>
                  (settings.density = settings.density === "compact" ? "comfortable" : "compact")}>
                {#if settings.density === "compact"}
                  <Rows4Icon class="size-4 shrink-0" />
                {:else}
                  <Rows3Icon class="size-4 shrink-0" />
                {/if}
                <span class={["truncate", sidebar.collapsed && "hidden"]}>Density</span>
                <span class={["ml-auto text-xs text-fg-subtle", sidebar.collapsed && "hidden"]}
                  >{DENSITY_LABELS[settings.density]}</span>
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
  <!-- the icon strip is one control wide plus the panel's padding -->
  <div
    class={[
      "h-full shrink-0 transition-[width] duration-(--motion-fast) ease-(--ease-out-soft)",
      sidebar.collapsed ? "w-[calc(var(--spacing-control)+1.5rem)]" : "w-60"
    ]}>
    {@render panel()}
  </div>
{/if}
