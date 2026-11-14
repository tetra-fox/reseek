<script lang="ts">
  import ClockIcon from "@lucide/svelte/icons/clock";
  import PlugIcon from "@lucide/svelte/icons/plug";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import StarIcon from "@lucide/svelte/icons/star";
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
  import XIcon from "@lucide/svelte/icons/x";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { UNREACHABLE_HINT } from "#lib/reachability.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";

  let { collapsed = false }: { collapsed?: boolean } = $props();

  const busy = new Busy();

  /** What the button shows and does, from the server connection */
  const view = $derived.by(() => {
    const state = app.state;
    const server = state?.server;
    const watchdog = state?.connectionWatchdog;
    if (server?.isConnected) {
      const cut = server.listen.reachable === "no";
      return {
        label: cut ? "Connected, unreachable" : "Connected",
        plug: state?.pendingReconnect || cut ? "text-warning-icon" : "text-success-icon",
        badge: cut
          ? ("unreachable" as const)
          : state?.user.privileges.isPrivileged
            ? ("privileged" as const)
            : null,
        connected: true
      };
    }
    return {
      label: "Disconnected",
      plug: "text-fg-subtle",
      badge:
        server?.isConnecting || server?.isLoggingIn
          ? ("connecting" as const)
          : watchdog?.isAttemptingConnection
            ? ("waiting" as const)
            : ("closed" as const),
      connected: false
    };
  });

  const toggle = () =>
    busy.run(() =>
      view.connected
        ? call(() => api.DELETE("/api/v0/server", { body: "client disconnected from web UI" }))
        : call(() => api.PUT("/api/v0/server"))
    );

  // connecting and disconnecting need ReadWrite; a ReadOnly user sees the state only
  const allowed = $derived(session.can("ReadWrite"));
  const hint = $derived(
    [
      view.badge === "unreachable" ? UNREACHABLE_HINT : null,
      allowed ? `${view.connected ? "Disconnect from" : "Connect to"} the Soulseek server` : null
    ]
      .filter(Boolean)
      .join(" ") || undefined
  );
</script>

<Tooltip text={collapsed ? [view.label, hint].filter(Boolean).join(". ") : hint} side="right">
  {#snippet child({ props })}
    <button
      {...props}
      type="button"
      onclick={toggle}
      disabled={!allowed || busy.running}
      aria-label={view.label}
      class="nav-item">
      <span class="relative shrink-0">
        <PlugIcon class={["size-4", view.plug]} />
        {#if view.badge === "unreachable"}
          <TriangleAlertIcon
            class="absolute -top-1 -right-1 size-2.5 fill-warning-icon text-warning-icon" />
        {:else if view.badge === "privileged"}
          <StarIcon class="absolute -top-1 -right-1 size-2.5 fill-warning-icon text-warning-icon" />
        {:else if view.badge === "connecting"}
          <RefreshCwIcon
            class="absolute -top-1 -right-1 size-2.5 animate-spin text-success-icon motion-reduce:animate-none" />
        {:else if view.badge === "waiting"}
          <ClockIcon class="absolute -top-1 -right-1 size-2.5 text-warning-icon" />
        {:else if view.badge === "closed"}
          <XIcon class="absolute -top-1 -right-1 size-2.5 text-danger" />
        {/if}
      </span>
      <span class={["truncate", collapsed && "hidden"]}>{view.label}</span>
    </button>
  {/snippet}
</Tooltip>
