<script lang="ts">
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import type { GroupAction } from "#lib/transfers/groupActions.js";

  let { actions }: { actions: GroupAction[] } = $props();
</script>

<!-- a group's actions as menu items: its ... button on a phone and a right-click on its line -->
{#each actions as a (a.label)}
  <Menu.Item onSelect={a.run}><a.icon />{a.label}</Menu.Item>
{/each}
