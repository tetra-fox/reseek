<script lang="ts">
  import { hasRtl, pathParts, splitTail } from "#lib/text.js";
  import { cn } from "#lib/ui/cn.js";

  type Props = {
    text: string;
    /** a path, which keeps its last part whole and draws each part as a run of its own */
    path?: boolean;
    class?: string;
  };

  let { text, path = false, class: className }: Props = $props();

  const [head, tail] = $derived(splitTail(text, path));
  // a right-to-left part swaps places with the parts around it unless each is a run of its own;
  // a path without one is left as plain text
  const isolated = $derived(path && hasRtl(text));
</script>

<!-- cut in the middle: the head gives way and takes the ellipsis, the tail never shrinks. No
     whitespace inside the spans, which keep it, so the text reads as the name it shows -->
{#snippet run(text: string)}{#if isolated}{#each pathParts(text) as part, i (i)}<bdi>{part}</bdi
      >{/each}{:else}{text}{/if}{/snippet}
<span class={cn("flex min-w-0", className)} dir={path ? "ltr" : "auto"}
  >{#if head}<span class="min-w-0 overflow-hidden text-ellipsis whitespace-pre"
      >{@render run(head)}</span
    >{/if}<span class="max-w-full shrink-0 overflow-hidden text-ellipsis whitespace-pre"
    >{@render run(tail)}</span
  ></span>
