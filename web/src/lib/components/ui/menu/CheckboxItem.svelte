<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import { DropdownMenu } from "bits-ui";

  import { itemClass } from "./Item.svelte";

  let { children: label, ...rest }: Omit<DropdownMenu.CheckboxItemProps, "class"> = $props();
</script>

<!-- an option that is on or off, its check on the right like the radio items' -->
<DropdownMenu.CheckboxItem class={[itemClass, "justify-between gap-6"]} {...rest}>
  {#snippet children({ checked, indeterminate })}
    {@render label?.({ checked, indeterminate })}
    {#if checked}<CheckIcon class="size-4 text-fg-muted" />{/if}
  {/snippet}
</DropdownMenu.CheckboxItem>
