<script lang="ts">
  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import { href } from "#lib/nav.js";
  import { SOULSEEK_SETTINGS, unreachable } from "#lib/reachability.js";

  // other users can't connect to us: said plainly where it costs the most, on searches
  const listen = $derived(app.state?.server.listen);
</script>

{#if listen?.reachable === "no"}
  <Notice kind="warning">
    {unreachable(listen.port)}
    {#if listen.portMapping.state === "failed" && listen.portMapping.error}
      <span class="mt-1 block text-fg-muted">
        Port mapping is on, but the router refused it: {listen.portMapping.error}
      </span>
    {/if}
    {#snippet action()}
      <Button variant="outline" size="sm" href={href(SOULSEEK_SETTINGS)}>Settings</Button>
    {/snippet}
  </Notice>
{/if}
