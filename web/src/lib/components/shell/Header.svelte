<script lang="ts">
  import { page } from "$app/state";
  import CheckIcon from "@lucide/svelte/icons/check";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
  import SearchIcon from "@lucide/svelte/icons/search";

  import { app } from "#lib/app.svelte.js";
  import { CopyButton } from "#lib/clipboard.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { trail } from "#lib/crumbs.svelte.js";
  import { baseCrumbs, href, route } from "#lib/nav.js";

  import NewVersion from "./NewVersion.svelte";
  import { palette } from "./palette.svelte";
  import { sidebar } from "./sidebar.svelte";

  const crumbs = $derived([...baseCrumbs(route(page.url).path), ...trail.page]);
  const copier = new CopyButton();
  const mac = typeof navigator !== "undefined" && /mac|iphone|ipad/i.test(navigator.platform);

  const pending = $derived.by(() => {
    const s = app.state;
    if (!s) return [];
    return [
      s.pendingRestart && "a restart",
      s.pendingReconnect && "a reconnect",
      s.shares.scanPending && "a share scan"
    ].filter((p): p is string => !!p);
  });
</script>

<header
  class="sticky top-0 z-10 flex h-14 shrink-0 items-center gap-inline border-b bg-page/75 px-gutter backdrop-blur-md">
  <Button variant="ghost" size="icon-sm" class="-ml-1.5" onclick={sidebar.toggle}>
    <PanelLeftIcon />
    <span class="sr-only">Toggle the navigation</span>
  </Button>
  <div class="mx-1 h-4 w-px bg-border"></div>
  <nav aria-label="Breadcrumb" class="min-w-0 flex-1">
    <ol class="flex min-w-0 items-center gap-1 text-sm">
      {#each crumbs as crumb, i (`${i}:${crumb.label}`)}
        {@const last = i === crumbs.length - 1}
        <li class={["flex min-w-0 items-center gap-1", !last && "max-sm:hidden"]}>
          {#if i > 0}<ChevronRightIcon class="size-3.5 shrink-0 text-fg-subtle" />{/if}
          {#if last}
            <span aria-current="page" class="truncate font-medium" title={crumb.copy}>
              {crumb.label}
            </span>
            {#if crumb.copy}
              {@const text = crumb.copy}
              <Button
                variant="ghost"
                size="icon-sm"
                class="size-6"
                aria-label="Copy the path"
                onclick={() => void copier.copy(text)}>
                {#if copier.copied}<CheckIcon class="text-success" />{:else}<CopyIcon />{/if}
              </Button>
            {/if}
          {:else if crumb.href}
            <a
              href={crumb.href}
              onclick={crumb.onclick}
              class="max-w-48 truncate rounded-badge px-1 text-fg-muted hover:bg-hover hover:text-fg"
              >{crumb.label}</a>
          {:else if crumb.onclick}
            <button
              type="button"
              onclick={crumb.onclick}
              class="max-w-48 truncate rounded-badge px-1 text-fg-muted hover:bg-hover hover:text-fg"
              >{crumb.label}</button>
          {:else}
            <span class="max-w-48 truncate px-1 text-fg-muted">{crumb.label}</span>
          {/if}
        </li>
      {/each}
    </ol>
  </nav>

  <div class="ml-auto flex items-center gap-inline">
    {#if pending.length}
      <Tooltip text={`Waiting on ${pending.join(", ")}`}>
        {#snippet child({ props })}
          <a
            {...props}
            href={href("/system/info")}
            class="flex h-control-sm items-center gap-1.5 rounded-control px-2 text-sm text-warning-soft-fg hover:bg-hover">
            <CircleAlertIcon class="size-4 text-warning" />
            <span class="hidden sm:inline">Pending Action</span>
          </a>
        {/snippet}
      </Tooltip>
    {/if}
    <NewVersion />
    <button
      type="button"
      class="flex h-control-sm items-center gap-2 rounded-control border bg-surface px-2 text-sm text-fg-muted shadow-xs hover:bg-hover hover:text-fg"
      aria-label="Go to a screen, user, room or setting"
      aria-keyshortcuts={mac ? "Meta+K" : "Control+K"}
      onclick={() => palette.show()}>
      <SearchIcon class="size-4" />
      <span class="hidden md:inline">Go to</span>
      <kbd class="hidden rounded-badge border px-1 font-sans text-xs md:inline"
        >{mac ? "⌘K" : "Ctrl K"}</kbd>
    </button>
  </div>
</header>
