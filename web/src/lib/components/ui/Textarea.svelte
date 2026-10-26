<script lang="ts">
  import type { HTMLTextareaAttributes } from "svelte/elements";

  import { cn } from "#lib/utils.js";

  let { value = $bindable(), class: className, ...rest }: HTMLTextareaAttributes = $props();
</script>

<textarea
  class={cn(
    "min-h-16 w-full rounded-control border border-input bg-surface px-3 py-2 text-base text-fg shadow-xs transition-colors duration-(--motion-fast) placeholder:text-fg-muted hover:border-fg-subtle/50 focus-visible:border-accent disabled:opacity-50 aria-invalid:border-danger md:text-sm",
    className
  )}
  bind:value
  {...rest}></textarea>
