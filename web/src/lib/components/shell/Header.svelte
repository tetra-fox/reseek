<script lang="ts">
  import { page } from "$app/state";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";

  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { live } from "#lib/live/live.svelte.js";
  import { href, route, title } from "#lib/nav.js";
  import { REFRESH_OPTIONS, settings } from "#lib/settings.svelte.js";

  import NewVersion from "./NewVersion.svelte";
  import { sidebar } from "./sidebar.svelte";

  const heading = $derived(title(route(page.url).path).split(" - ").slice(0, -1).reverse());

  const every = $derived(REFRESH_OPTIONS.find((o) => o.value === settings.refresh)?.label);
  // how the page keeps up right now, and the menu under it picks how it should
  const status = $derived(
    {
      connected: { dot: "bg-success shadow-[0_0_0_3px] shadow-success/20", label: "Live" },
      connecting: { dot: "bg-fg-muted animate-pulse", label: "Connecting" },
      polling: {
        dot: "bg-warning",
        label: settings.refresh === "live" ? "Polling, live unavailable" : `Every ${every}`
      },
      off: { dot: "bg-fg-muted/40", label: "Paused" }
    }[live.status]
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
    {#each heading as part, i (i)}
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
    <Menu.Root>
      <Menu.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="outline"
            class="rounded-full pr-2 pl-3 font-normal text-fg-muted"
            aria-label={`Updates: ${status.label}`}>
            <span class={`size-2 rounded-full ${status.dot}`}></span>
            <span class="hidden sm:inline">{status.label}</span>
            <ChevronDownIcon class="text-fg-subtle" />
          </Button>
        {/snippet}
      </Menu.Trigger>
      <Menu.Content align="end" class="min-w-44">
        <Menu.Label>Keep the page current</Menu.Label>
        <!-- bits-ui values are strings, so options are keyed by index -->
        <Menu.RadioGroup
          value={String(REFRESH_OPTIONS.findIndex((o) => o.value === settings.refresh))}
          onValueChange={(i) => (settings.refresh = REFRESH_OPTIONS[Number(i)].value)}>
          {#each REFRESH_OPTIONS as option, i (option.label)}
            <Menu.RadioItem value={String(i)}>
              {typeof option.value === "number" ? `Every ${option.label}` : option.label}
            </Menu.RadioItem>
          {/each}
        </Menu.RadioGroup>
      </Menu.Content>
    </Menu.Root>
  </div>
</header>
