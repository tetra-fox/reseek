<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";

  import { cn } from "#lib/utils.js";

  let { value = $bindable(), type, class: className, ...rest }: HTMLInputAttributes = $props();
</script>

<!-- text-base below md keeps iOS from zooming in on focus -->
<input
  class={cn(
    "h-control w-full min-w-0 rounded-control border border-input bg-surface px-3 text-base text-fg shadow-xs transition-colors duration-(--motion-fast) placeholder:text-fg-muted hover:border-fg-subtle/50 focus-visible:border-accent disabled:opacity-50 aria-invalid:border-danger md:text-sm",
    className
  )}
  {type}
  bind:value
  {...rest} />
