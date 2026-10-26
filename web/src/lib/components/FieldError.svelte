<script lang="ts">
  import { problems } from "#lib/components/Form.svelte";

  // id of the field this shows the validation message for
  let { for: id }: { for: string } = $props();

  const list = problems();
  const problem = $derived(list().find((p) => p.id === id));
</script>

{#if problem}
  <p class="text-xs text-danger" role="alert">{problem.message}</p>
{/if}
