<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";

  // a user's upload group, with the colour the daemon gives the group
  let { group }: { group: Schemas["UserGroup"] } = $props();
</script>

<Badge tone="outline" title={`Upload group ${group.name}`}>
  {#if group.color}
    <span class="size-2 rounded-full" style:background-color={group.color}></span>
  {/if}
  {group.name}
</Badge>
