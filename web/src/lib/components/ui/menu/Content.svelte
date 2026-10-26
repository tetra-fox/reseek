<script lang="ts">
  import { DropdownMenu } from "bits-ui";

  import { cn } from "#lib/utils.js";

  let { sideOffset = 4, class: className, ...rest }: DropdownMenu.ContentProps = $props();
</script>

<DropdownMenu.Portal>
  <DropdownMenu.Content
    {sideOffset}
    class={cn(
      "z-50 max-h-(--bits-dropdown-menu-content-available-height) max-w-(--bits-dropdown-menu-content-available-width) min-w-32 origin-(--bits-dropdown-menu-content-transform-origin) overflow-y-auto rounded-control border bg-overlay p-1 text-fg shadow-raised outline-none data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in",
      className
    )}
    {...rest} />
</DropdownMenu.Portal>
