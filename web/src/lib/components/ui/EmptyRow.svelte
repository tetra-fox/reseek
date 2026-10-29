<script lang="ts">
  import { cn } from "#lib/ui/cn.js";

  type Props = {
    /** what the list would hold, said as missing: "No log lines" */
    text: string;
    /** the table's column count, for an empty line inside a table */
    colspan?: number;
    class?: string;
  };

  // an empty line inside a list or a table; a whole panel with nothing in it is an EmptyState
  let { text, colspan, class: className }: Props = $props();

  const style = $derived(cn("px-4 py-6 text-center text-sm text-fg-subtle", className));
</script>

{#if colspan}
  <tr><td {colspan} class={style}>{text}</td></tr>
{:else}
  <p class={style}>{text}</p>
{/if}
