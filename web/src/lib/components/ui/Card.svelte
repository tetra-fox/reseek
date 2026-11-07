<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";

  import { cn } from "#lib/ui/cn.js";

  type Props = Omit<HTMLAttributes<HTMLElement>, "title"> & {
    title?: string;
    description?: string | Snippet;
    action?: Snippet;
    // rendered after the body, edge to edge, for tables
    flush?: Snippet;
  };

  let { title, description, action, flush, class: className, children, ...rest }: Props = $props();
</script>

<!-- a section: its heading and action above a framed box that holds the content -->
<section class={cn("flex flex-col gap-3 text-fg", className)} {...rest}>
  {#if title || description || action}
    <!-- the action goes under the title when large text leaves no room beside it. The line is
         as tall as a small control, so headings beside each other line up with or without one -->
    <div
      class={[
        "flex min-h-control-sm flex-wrap justify-between gap-inline",
        description ? "items-end" : "items-center"
      ]}>
      <div class="flex min-w-0 flex-col gap-0.5">
        {#if title}
          <h3 class="text-base leading-tight font-semibold tracking-tight select-none">{title}</h3>
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
  <!-- clipped rather than hidden, so a sticky element inside still sticks -->
  <div class="flex flex-1 flex-col overflow-clip rounded-card border bg-surface">
    {#if children}
      <!-- a snippet can render nothing, like a form behind an if, and an empty body would add a
           gap. :empty ignores the comment nodes svelte leaves where a block rendered nothing. A
           list marked data-edge runs to the frame, and at the top or bottom also meets it -->
      <div
        class="flex flex-1 flex-col gap-group p-5 empty:hidden [&>[data-edge]:first-child]:-mt-5 [&>[data-edge]:first-child]:border-t-0 [&>[data-edge]:last-child]:-mb-5 [&>[data-edge]:last-child]:border-b-0">
        {@render children()}
      </div>
    {/if}
    {@render flush?.()}
  </div>
</section>
