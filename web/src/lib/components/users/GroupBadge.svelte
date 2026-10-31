<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";

  // a user's upload group, with the color the daemon gives the group
  let { group, class: className }: { group: Schemas["UserGroup"]; class?: string } = $props();
</script>

<Badge tone="outline" class={className} title={`Upload group ${group.name}`}>
  {#if group.color}
    <span class="size-2 rounded-full" style:background-color={group.color}></span>
  {/if}
  {group.name}
</Badge>
