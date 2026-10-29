<script lang="ts">
  import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";

  import { cn } from "#lib/ui/cn.js";

  // a turning circle for something under way, with the words a screen reader hears
  let { label = "Loading", class: className }: { label?: string; class?: string } = $props();
</script>

<span role="status" class={cn("inline-flex items-center text-fg-muted", className)}>
  <LoaderCircleIcon class="size-full animate-spin motion-reduce:animate-none" aria-hidden="true" />
  <span class="sr-only">{label}</span>
</span>
