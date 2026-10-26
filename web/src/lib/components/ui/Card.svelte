<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";

  import { cn } from "#lib/utils.js";

  type Props = Omit<HTMLAttributes<HTMLDivElement>, "title"> & {
    title?: string;
    description?: string | Snippet;
    action?: Snippet;
    // rendered after the body, edge to edge, for tables
    flush?: Snippet;
  };

  let { title, description, action, flush, class: className, children, ...rest }: Props = $props();
</script>

<div
  class={cn(
    "flex flex-col gap-group rounded-card border bg-surface pt-5 text-fg shadow-card",
    flush ? "overflow-hidden" : "pb-5",
    className
  )}
  {...rest}>
  {#if title || description || action}
    <div class="flex items-start justify-between gap-inline px-5">
      <div class="flex min-w-0 flex-col gap-1">
        {#if title}
          <h3 class="text-sm leading-tight font-semibold select-none">{title}</h3>
        {/if}
        {#if typeof description === "function"}
          <p class="text-sm text-fg-muted">{@render description()}</p>
        {:else if description}
          <p class="text-sm text-fg-muted">{description}</p>
        {/if}
      </div>
      {@render action?.()}
    </div>
  {/if}
  {#if children}
    <!-- a snippet can render nothing, like a form behind an if, and an empty body would add a gap.
         :empty ignores the comment nodes svelte leaves where a block rendered nothing -->
    <div class="flex flex-1 flex-col gap-group px-5 empty:hidden">{@render children()}</div>
  {/if}
  {@render flush?.()}
</div>
