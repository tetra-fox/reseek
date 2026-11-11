<script lang="ts">
  import { type Binding, capsOf } from "#lib/ui/keys.js";

  type Props = { binding: Binding; class?: string };

  let { binding, class: className }: Props = $props();
</script>

<!-- a binding's keys as key caps, as the keyboard in use names them, with "or" between the
     chords that do the same -->
<span class={["inline-flex flex-wrap items-center gap-x-1 gap-y-0.5", className]}>
  {#if binding.typed}
    <span>{binding.typed}</span>
  {:else}
    {#each binding.keys.map((chord) => capsOf(chord)) as caps, i (caps.join(" "))}
      {#if i}<span class="text-fg-subtle">or</span>{/if}
      <span class="inline-flex gap-0.5">
        {#each caps as cap (cap)}
          <kbd
            class="min-w-5 rounded-badge border bg-surface px-1 text-center font-sans text-xs text-fg"
            >{cap}</kbd>
        {/each}
      </span>
    {/each}
  {/if}
</span>
