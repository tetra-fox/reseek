<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { untrack } from "svelte";
  import { fly } from "svelte/transition";

  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import CopyIcon from "#lib/components/ui/CopyIcon.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { CopyButton } from "#lib/feedback/clipboard.svelte.js";
  import { listText } from "#lib/format.js";
  import { baseCrumbs, type Crumb, href, route } from "#lib/nav.js";
  import { trail } from "#lib/shell/crumbs.svelte.js";
  import { palette } from "#lib/shell/palette.svelte.js";
  import { sidebar } from "#lib/shell/sidebar.svelte.js";
  import { shortcutAria, shortcutText } from "#lib/ui/keys.js";
  import { easeOutSoft, motionFast } from "#lib/ui/motion.js";

  import NewVersion from "./NewVersion.svelte";

  const base = $derived(baseCrumbs(route(page.url).path));
  const crumbs = $derived([...base, ...trail.page]);
  /**
   * past this many steps the middle ones fold into a menu: the screen's own steps and the first
   * after them, which says whose page it is, stay, and so do the last two
   */
  const MAX_SHOWN = 7;
  const folded = $derived(
    crumbs.length > MAX_SHOWN ? crumbs.slice(base.length + 1, crumbs.length - 2) : []
  );
  const follow = (crumb: Crumb) => {
    if (crumb.onclick) crumb.onclick();
    else if (crumb.href) void goto(crumb.href);
  };
  const copier = new CopyButton();

  const keyOf = (crumb: Crumb, i: number) => `${i}:${crumb.label}`;

  let shown = untrack(() => crumbs.map(keyOf));
  let changedAt = -Infinity;
  let entering = false;

  // a crumb comes in moving only once the trail has held for 300ms, as browse rewrites it for each
  // row a held arrow key passes
  const moving = () => {
    const keys = crumbs.map(keyOf);
    if (keys.length !== shown.length || keys.some((k, i) => k !== shown[i])) {
      const now = performance.now();
      entering = now - changedAt >= 300;
      shown = keys;
      changedAt = now;
    }
    return entering;
  };
  const step = (on: boolean) => ({ x: -4, duration: on ? motionFast() : 0, easing: easeOutSoft });

  const pending = $derived.by(() => {
    const s = app.state;
    if (!s) return [];
    return [
      s.pendingRestart && "a restart",
      s.pendingReconnect && "a reconnect",
      s.shares.scanPending && "a share rescan"
    ].filter((p): p is string => !!p);
  });
</script>

<header
  class="sticky top-0 z-10 flex h-header shrink-0 items-center gap-inline border-b bg-surface px-gutter">
  <Tooltip text="Toggle navigation">
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="icon-sm" class="-ml-1.5" onclick={sidebar.toggle}>
        <PanelLeftIcon />
        <span class="sr-only">Toggle navigation</span>
      </Button>
    {/snippet}
  </Tooltip>
  <div class="mx-1 h-4 w-px bg-border"></div>
  <nav aria-label="Breadcrumb" class="min-w-0 flex-1">
    <ol class="flex min-w-0 items-center gap-1 text-sm">
      {#each crumbs as crumb, i (keyOf(crumb, i))}
        {@const last = i === crumbs.length - 1}
        {@const hidden = folded.includes(crumb)}
        {#if hidden && crumb === folded[0]}
          <li class="flex shrink-0 items-center gap-1 max-sm:hidden">
            <ChevronRightIcon class="size-3.5 shrink-0 text-fg-subtle" />
            <Menu.Root>
              <Menu.Trigger>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    class="size-6"
                    aria-label={`${folded.length} more steps`}>
                    <EllipsisIcon />
                  </Button>
                {/snippet}
              </Menu.Trigger>
              <Menu.Content align="start" class="max-w-[min(32rem,calc(100vw-1rem))]">
                {#each folded as step, j (keyOf(step, j))}
                  <Menu.Item onSelect={() => follow(step)}>
                    <span class="truncate">{step.label}</span>
                  </Menu.Item>
                {/each}
              </Menu.Content>
            </Menu.Root>
          </li>
        {/if}
        {#if !hidden}
          <!-- a leaving crumb goes at once: svelte 5.57 never removes a batch of leaving each items
             whose outros mix instant and animated ones, as a crumb's cached outro could here, so
             they stayed on screen (https://github.com/sveltejs/svelte/issues/19041). TODO: give
             crumbs an outro again once a release fixes it -->
          <!-- the screen's own steps keep their width, so a long last step never squeezes them -->
          <li
            class={[
              "flex items-center gap-1",
              i < base.length && !last ? "shrink-0" : "min-w-0",
              !last && "max-sm:hidden"
            ]}
            in:fly={step(moving())}>
            <!-- a phone shows only the last step, so it has no step before it to point from -->
            {#if i > 0}<ChevronRightIcon
                class="size-3.5 shrink-0 text-fg-subtle max-sm:hidden" />{/if}
            {#if last}
              <Tooltip text={crumb.copy} clipped={crumb.copy === crumb.label}>
                {#snippet child({ props })}
                  <span
                    {...props}
                    aria-current="page"
                    data-text={crumb.label}
                    class="weight-steady truncate px-1 font-medium">
                    {crumb.label}
                  </span>
                {/snippet}
              </Tooltip>
              {#if crumb.copy}
                {@const text = crumb.copy}
                <Tooltip text="Copy path">
                  {#snippet child({ props })}
                    <Button
                      {...props}
                      variant="ghost"
                      size="icon-sm"
                      class="size-6"
                      aria-label="Copy path"
                      onclick={() => void copier.copy(text)}>
                      <CopyIcon copied={copier.copied} class="text-success" />
                    </Button>
                  {/snippet}
                </Tooltip>
              {/if}
            {:else}
              <Tooltip text={crumb.label} clipped>
                {#snippet child({ props })}
                  {#if crumb.href}
                    <a
                      {...props}
                      href={crumb.href}
                      onclick={crumb.onclick}
                      data-text={crumb.label}
                      class="weight-steady max-w-48 truncate rounded-badge px-1 text-fg-muted hover:bg-hover hover:text-fg"
                      >{crumb.label}</a>
                  {:else if crumb.onclick}
                    <button
                      {...props}
                      type="button"
                      onclick={crumb.onclick}
                      data-text={crumb.label}
                      class="weight-steady max-w-48 truncate rounded-badge px-1 text-fg-muted hover:bg-hover hover:text-fg"
                      >{crumb.label}</button>
                  {:else}
                    <span
                      {...props}
                      data-text={crumb.label}
                      class="weight-steady max-w-48 truncate px-1 text-fg-muted"
                      >{crumb.label}</span>
                  {/if}
                {/snippet}
              </Tooltip>
            {/if}
          </li>
        {/if}
      {/each}
    </ol>
  </nav>

  <div class="ml-auto flex items-center gap-inline">
    {#if pending.length}
      <Tooltip text={`Some changes take effect after ${listText(pending)}`}>
        {#snippet child({ props })}
          <a
            {...props}
            href={href("/system/info")}
            class="flex h-control-sm items-center gap-1.5 rounded-control px-2 text-sm text-warning-soft-fg hover:bg-hover">
            <CircleAlertIcon class="size-4 text-warning" />
            <span class="max-sm:sr-only">Changes pending</span>
          </a>
        {/snippet}
      </Tooltip>
    {/if}
    <NewVersion />
    <button
      type="button"
      class="flex h-control-sm items-center gap-2 rounded-control border bg-surface px-2 text-sm text-fg-muted shadow-xs hover:bg-hover hover:text-fg"
      aria-label="Go to a screen, user, room or setting"
      aria-keyshortcuts={shortcutAria("K")}
      onclick={() => palette.show()}>
      <SearchIcon class="size-4" />
      <span class="hidden md:inline">Go to</span>
      <kbd class="hidden rounded-badge border px-1 font-sans text-xs md:inline"
        >{shortcutText("K")}</kbd>
    </button>
  </div>
</header>
