<script lang="ts">
  import type { Snippet } from "svelte";

  import FieldError from "#lib/components/FieldError.svelte";
  import { cn } from "#lib/ui/cn.js";

  type Props = {
    label: string;
    /**
     * id of the control, which the label and the validation message point at. A control a label
     * can't point at, like a segmented control, goes without and takes the label as its aria-label
     */
    for?: string;
    /** shown under the control, which points at it with `aria-describedby="<for>-hint"` */
    hint?: string;
    class?: string;
    children: Snippet;
  };

  let { label, for: id, hint, class: className, children }: Props = $props();
</script>

<div class={cn("flex flex-col gap-1.5", className)}>
  <!-- without a control to point at it's hidden from screen readers, which hear the aria-label -->
  <svelte:element
    this={id ? "label" : "span"}
    for={id}
    aria-hidden={id ? undefined : "true"}
    class="text-xs font-medium text-fg-muted">
    {label}
  </svelte:element>
  {@render children()}
  {#if id}<FieldError for={id} />{/if}
  {#if hint}<p id={id && `${id}-hint`} class="text-xs text-pretty text-fg-muted">{hint}</p>{/if}
</div>
