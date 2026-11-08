<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // a user's upload group, with the color the daemon gives the group
  let { group, class: className }: { group: Schemas["UserGroup"]; class?: string } = $props();
</script>

<Tooltip text={`Upload group ${group.name}`} dense>
  {#snippet child({ props })}
    <Badge {...props} tone="outline" class={className}>
      {#if group.color}
        <span
          class="mr-1 inline-block size-2 rounded-full align-middle"
          style:background-color={group.color}></span>
      {/if}
      {group.name}
    </Badge>
  {/snippet}
</Tooltip>
