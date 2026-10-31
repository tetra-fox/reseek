<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import type { Snippet } from "svelte";

  import { inspectorPrefs, type SectionId } from "#lib/inspector/inspector.svelte.js";
  import { disclose } from "#lib/ui/motion.js";

  type Props = { id: SectionId; title: string; children: Snippet };

  let { id, title, children }: Props = $props();
</script>

<!-- a part of the inspector that folds, open or closed as it was last left on any file -->
<section class="border-t">
  <h3>
    <button
      type="button"
      class="flex w-full items-center gap-1.5 px-4 py-2 text-left text-sm font-medium"
      aria-expanded={inspectorPrefs.sections[id]}
      onclick={() => (inspectorPrefs.sections[id] = !inspectorPrefs.sections[id])}>
      <ChevronRightIcon
        class={[
          "size-4 text-fg-muted transition-transform duration-(--motion-fast)",
          inspectorPrefs.sections[id] && "rotate-90"
        ]} />
      {title}
    </button>
  </h3>
  {#if inspectorPrefs.sections[id]}
    <div class="text-sm" transition:disclose>
      <div class="px-4 pb-3">{@render children()}</div>
    </div>
  {/if}
</section>
