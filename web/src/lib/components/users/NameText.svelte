<script lang="ts">
  // a username as text with each space at either end drawn as a faint dot, since `dj ` and `dj`
  // are different users. The dots are hidden from screen readers, so a link or heading around it
  // takes the name as its label
  let { name }: { name: string } = $props();

  // the pattern matches every string
  const edges = $derived(/^(\s*)(.*?)(\s*)$/s.exec(name)!);
</script>

{#if edges[1]}<span class="opacity-40" aria-hidden="true">{"·".repeat(edges[1].length)}</span
  >{/if}{edges[2]}{#if edges[3]}<span class="opacity-40" aria-hidden="true"
    >{"·".repeat(edges[3].length)}</span
  >{/if}
