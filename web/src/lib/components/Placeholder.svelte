<script lang="ts">
  import ConstructionIcon from "@lucide/svelte/icons/construction";

  import EmptyState from "#lib/components/ui/EmptyState.svelte";

  // a screen whose route exists but whose content is not built yet; it never shows made-up data
  let { screen }: { screen: string } = $props();
</script>

<EmptyState
  icon={ConstructionIcon}
  title={`${screen} isn't built yet`}
  description={`The ${screen.toLowerCase()} screen comes in a later version of this UI.`} />
