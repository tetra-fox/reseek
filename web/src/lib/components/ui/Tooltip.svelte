<script lang="ts">
  import { Tooltip } from "bits-ui";
  import type { Snippet } from "svelte";

  import { type Side, tether } from "#lib/components/ui/TooltipHost.svelte";

  type Props = {
    /** no tooltip when it's missing */
    text: string | undefined;
    /** which side of the element it opens on */
    side?: Side;
    /** for static elements: keeps them out of the tab order and repeats the text for screen readers */
    hint?: boolean;
    /** the element the tooltip belongs to, which spreads `props` */
    child: Snippet<[{ props: Record<string, unknown> }]>;
  };

  let { text, side = "top", hint = false, child: render }: Props = $props();
</script>

<!-- always a trigger, disabled without text, so the element isn't remounted when a tooltip comes
     and goes -->
<Tooltip.Trigger
  {tether}
  payload={{ text: text ?? "", side }}
  disabled={text === undefined}
  tabindex={hint ? -1 : undefined}>
  {#snippet child({ props })}
    <!-- type is for bits-ui's own button, and disabled only means there's nothing to show -->
    {@const {
      "type": _type,
      "disabled": _disabled,
      "data-disabled": _dataDisabled,
      ...own
    } = props}
    {@render render({ props: own })}
  {/snippet}
</Tooltip.Trigger>
{#if hint && text !== undefined}<span class="sr-only">{text}</span>{/if}
