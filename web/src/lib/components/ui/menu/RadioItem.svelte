<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import { DropdownMenu } from "bits-ui";

  import { itemClass } from "./Item.svelte";

  let { children: label, ...rest }: Omit<DropdownMenu.RadioItemProps, "class"> = $props();
</script>

<!-- the check sits on the right, like the select's options -->
<DropdownMenu.RadioItem class={[itemClass, "justify-between gap-6"]} {...rest}>
  {#snippet children({ checked })}
    {@render label?.({ checked })}
    {#if checked}<CheckIcon class="size-4 text-fg-muted" />{/if}
  {/snippet}
</DropdownMenu.RadioItem>
