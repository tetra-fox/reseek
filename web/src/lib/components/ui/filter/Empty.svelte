<script lang="ts">
  import FunnelXIcon from "@lucide/svelte/icons/funnel-x";

  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import { type ActiveFilter, clearThen, hiding, type Noun } from "#lib/lists/filtered.js";

  // where a list's rows would be when its filters hide every one, saying so and offering the
  // way back
  type Props = {
    filters: ActiveFilter[];
    /** everything the list holds; null for a list that counts only what matched */
    total: number | null;
    noun: Noun;
    /** gives focus to the list as it comes back */
    refocus: () => void;
    /** turns every filter off at once, where turning each off in turn does not */
    onclear?: () => void;
    class?: string;
  };

  let { filters, total, noun, refocus, onclear, class: className }: Props = $props();
</script>

<EmptyState icon={FunnelXIcon} title={hiding(total, noun)} class={className}>
  {#snippet action()}
    <Button variant="outline" onclick={() => void clearThen(filters, onclear, refocus)}
      >Clear filters</Button>
  {/snippet}
</EmptyState>
