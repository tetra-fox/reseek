<script lang="ts">
  import { cn } from "#lib/ui/cn.js";

  // a placeholder shaped like content that hasn't loaded yet
  let { class: className }: { class?: string } = $props();
</script>

<div class={cn("animate-pulse rounded-control bg-surface-muted", className)} aria-hidden="true">
</div>
