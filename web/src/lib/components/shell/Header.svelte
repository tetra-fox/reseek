<script lang="ts">
  import { page } from "$app/state";
  import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";

  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { live } from "#lib/live/live.svelte.js";
  import { href, route, title } from "#lib/nav.js";

  import NewVersion from "./NewVersion.svelte";
  import { sidebar } from "./sidebar.svelte";

  const heading = $derived(title(route(page.url).path).split(" - ").slice(0, -1).reverse());

  // the stream is always used; while it is down the screens poll until it is back
  const status = $derived(
    live.status === "connected"
      ? { dot: "bg-success shadow-[0_0_0_3px] shadow-success/20", label: "Live", hint: undefined }
      : live.status === "polling"
        ? {
            dot: "bg-warning",
            label: "Polling",
            hint: "The live connection is down: the page asks every few seconds until it is back"
          }
        : { dot: "bg-fg-muted animate-pulse", label: "Reconnecting", hint: undefined }
  );

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
  <h1 class="flex min-w-0 items-center gap-1.5 text-sm select-none">
    {#each heading as part, i (part)}
      {#if i > 0}<span class="text-fg-subtle">/</span>{/if}
      <span class={["truncate", i === heading.length - 1 ? "font-medium" : "text-fg-muted"]}>
        {part}
      </span>
    {/each}
  </h1>

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
    <Tooltip text={status.hint} hint>
      {#snippet child({ props })}
        <span
          {...props}
          role="status"
          aria-label={`Updates: ${status.label}`}
          class="flex h-control items-center gap-2 rounded-full border px-3 text-sm text-fg-muted">
          <span class={`size-2 rounded-full ${status.dot}`}></span>
          <span class="hidden sm:inline">{status.label}</span>
        </span>
      {/snippet}
    </Tooltip>
  </div>
</header>
