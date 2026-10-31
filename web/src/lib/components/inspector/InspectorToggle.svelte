<script lang="ts">
  import PanelRightIcon from "@lucide/svelte/icons/panel-right";

  import Button from "#lib/components/ui/Button.svelte";
  import { inspector } from "#lib/inspector/inspector.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
</script>

<!-- a phone's inspector is a sheet opened from a row, so it has no pane to show or hide here -->
{#if !viewport.phone}
  <Button
    variant={inspector.open ? "secondary" : "outline"}
    size="sm"
    aria-pressed={inspector.open}
    title="Show or hide the details of the file in focus (I or Space in the list)"
    onclick={() => inspector.toggle()}><PanelRightIcon />Details</Button>
{/if}
