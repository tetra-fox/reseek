<script lang="ts" module>
  // the highlight fill marks the focused item, so it gets no focus ring
  export const itemClass =
    "flex h-control-sm items-center gap-2 rounded-badge px-2 text-sm whitespace-nowrap outline-none select-none data-disabled:opacity-50 data-highlighted:bg-hover [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4 [&_svg:not([class*='text-'])]:text-fg-muted";
</script>

<script lang="ts">
  import { DropdownMenu } from "bits-ui";

  import { cn } from "#lib/utils.js";

  let { class: className, ...rest }: DropdownMenu.ItemProps = $props();
</script>

<DropdownMenu.Item class={cn(itemClass, className)} {...rest} />
