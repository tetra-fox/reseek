<script lang="ts">
  import { app } from "#lib/app.svelte.js";
  import { mappingText, reachableText } from "#lib/reachability.js";

  // whether the router forwards our port and whether other users get through, live from the state
  const listen = $derived(app.state?.server.listen);
</script>

{#if listen}
  <div
    class={[
      "flex flex-col gap-0.5 rounded-control border px-3 py-2 text-sm",
      listen.portMapping.state === "failed" || listen.reachable === "no"
        ? "border-warning/40 bg-warning-soft"
        : "bg-surface-muted"
    ]}
    role="status"
    aria-label="Port mapping">
    <p>{mappingText(listen)}</p>
    <p class="text-fg-muted">{reachableText(listen)}</p>
  </div>
{/if}
