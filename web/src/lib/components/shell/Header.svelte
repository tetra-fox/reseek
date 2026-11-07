<script lang="ts">
  import { page } from "$app/state";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { untrack } from "svelte";
  import { fly } from "svelte/transition";

  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import CopyIcon from "#lib/components/ui/CopyIcon.svelte";
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

  const crumbs = $derived([...baseCrumbs(route(page.url).path), ...trail.page]);
  const copier = new CopyButton();

  const keyOf = (crumb: Crumb, i: number) => `${i}:${crumb.label}`;

  let shown = untrack(() => crumbs.map(keyOf));
  let changedAt = -Infinity;
  let moves = { enter: false, leave: false };

  // crumbs move only once the trail has held for 300ms, as browse rewrites it for each row a held
  // arrow key passes; one leaving as another enters goes at once, so it never pushes that one aside
  const moving = () => {
    const keys = crumbs.map(keyOf);
    if (keys.length !== shown.length || keys.some((k, i) => k !== shown[i])) {
      const now = performance.now();
      const settled = now - changedAt >= 300;
      moves = { enter: settled, leave: settled && keys.every((k) => shown.includes(k)) };
      shown = keys;
      changedAt = now;
    }
    return moves;
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
  class="sticky top-0 z-10 flex h-14 shrink-0 items-center gap-inline border-b bg-surface px-gutter">
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
        <!-- svelte makes a leaving crumb inert, so it takes no clicks, and a phone hides it to
        show the new last one alone -->
        <li
          class={[
            "flex min-w-0 items-center gap-1 max-sm:[&[inert]]:hidden",
            !last && "max-sm:hidden"
          ]}
          in:fly={step(moving().enter)}
          out:fly={step(moving().leave)}>
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
              <Tooltip text="Copy the path">
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    class="size-6"
                    aria-label="Copy the path"
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
                    class="weight-steady max-w-48 truncate px-1 text-fg-muted">{crumb.label}</span>
                {/if}
              {/snippet}
            </Tooltip>
          {/if}
        </li>
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
            <span class="hidden sm:inline">Changes pending</span>
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
