<script lang="ts">
  import PanelRightIcon from "@lucide/svelte/icons/panel-right";

  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { inspector } from "#lib/inspector/inspector.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
</script>

<!-- a phone's inspector is a sheet opened from a row, so it has no pane to show or hide here -->
{#if !viewport.phone}
  <Tooltip text="File details (I or Space)">
    {#snippet child({ props })}
      <Button
        {...props}
        variant={inspector.open ? "secondary" : "outline"}
        size="sm"
        aria-pressed={inspector.open}
        onclick={() => inspector.toggle()}><PanelRightIcon />Details</Button>
    {/snippet}
  </Tooltip>
{/if}
