<script lang="ts">
  import { cn } from "#lib/ui/cn.js";

  import logo from "../../../../static/favicon.svg?raw";

  let { class: className }: { class?: string } = $props();
</script>

<!-- the app's icon, the tab's own file drawn inline so it takes the app's theme: as an image it
     would answer prefers-color-scheme from the system in Safari -->
<span aria-hidden="true" class={cn("block size-control shrink-0 *:block *:size-full", className)}
  >{@html logo}</span>
