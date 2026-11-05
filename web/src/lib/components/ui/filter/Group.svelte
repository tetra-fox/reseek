<script lang="ts">
  import type { Snippet } from "svelte";

  import { cn } from "#lib/ui/cn.js";

  // filter controls that belong together, after the name they go by, which also names the group
  // for a screen reader
  type Props = { label: string; class?: string; children: Snippet };

  let { label, class: className, children }: Props = $props();

  const id = $props.id();
</script>

<div class={cn("flex flex-wrap items-center gap-1", className)} role="group" aria-labelledby={id}>
  <span {id} class="mr-1 text-xs font-medium text-fg-muted">{label}</span>
  {@render children()}
</div>
