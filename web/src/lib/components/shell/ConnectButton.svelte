<script lang="ts">
  import ClockIcon from "@lucide/svelte/icons/clock";
  import PlugIcon from "@lucide/svelte/icons/plug";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import StarIcon from "@lucide/svelte/icons/star";
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
  import XIcon from "@lucide/svelte/icons/x";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { Busy } from "#lib/busy.svelte.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { unreachable } from "#lib/reachability.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";

  let { collapsed = false }: { collapsed?: boolean } = $props();

  const busy = new Busy();

  /** What the button shows and does, from the server connection or, on an agent, the controller's */
  const view = $derived.by(() => {
    const state = app.state;
    if (app.mode === "agent") {
      const controller = state?.relay.controller.state ?? "Disconnected";
      const connected = controller === "Connected";
      return {
        label: `Controller ${controller}`,
        plug: connected
          ? "text-success"
          : controller === "Connecting" || controller === "Reconnecting"
            ? "text-warning"
            : "text-fg-subtle",
        badge: connected ? null : ("closed" as const),
        connected
      };
    }
    const server = state?.server;
    const watchdog = state?.connectionWatchdog;
    if (server?.isConnected) {
      const cut = server.listen.reachable === "no";
      return {
        label: cut ? "Connected, unreachable" : "Connected",
        plug: state?.pendingReconnect || cut ? "text-warning" : "text-success",
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
    busy.run(() => {
      if (app.mode === "agent") {
        return view.connected
          ? call(() => api.DELETE("/api/v0/relay/agent"))
          : call(() => api.PUT("/api/v0/relay/agent"));
      }
      return view.connected
        ? call(() => api.DELETE("/api/v0/server", { body: "client disconnected from web UI" }))
        : call(() => api.PUT("/api/v0/server"));
    });

  // connecting and disconnecting need ReadWrite; a ReadOnly user sees the state only
  const allowed = $derived(session.can("ReadWrite"));
  const listen = $derived(app.state?.server.listen);
  const hint = $derived(
    [
      view.badge === "unreachable" && listen ? unreachable(listen.port) : null,
      allowed ? (view.connected ? "Click to disconnect" : "Click to connect") : null
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
      class="flex h-control w-full items-center gap-2.5 rounded-control px-2 text-sm whitespace-nowrap text-fg-muted transition-colors duration-(--motion-fast) not-disabled:hover:bg-hover not-disabled:hover:text-fg">
      <span class="relative shrink-0">
        <PlugIcon class={["size-4", view.plug]} />
        {#if view.badge === "unreachable"}
          <TriangleAlertIcon class="absolute -top-1 -right-1 size-2.5 fill-warning text-warning" />
        {:else if view.badge === "privileged"}
          <StarIcon class="absolute -top-1 -right-1 size-2.5 fill-warning text-warning" />
        {:else if view.badge === "connecting"}
          <RefreshCwIcon
            class="absolute -top-1 -right-1 size-2.5 animate-spin text-success motion-reduce:animate-none" />
        {:else if view.badge === "waiting"}
          <ClockIcon class="absolute -top-1 -right-1 size-2.5 text-warning" />
        {:else if view.badge === "closed"}
          <XIcon class="absolute -top-1 -right-1 size-2.5 text-danger" />
        {/if}
      </span>
      <span class={["truncate", collapsed && "hidden"]}>{view.label}</span>
    </button>
  {/snippet}
</Tooltip>
