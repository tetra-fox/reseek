<script lang="ts">
  import Button from "#lib/components/ui/Button.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";

  // why the last load failed, shown over whatever loaded before it
  let { loader }: { loader: { error: string; load: () => Promise<void> } } = $props();
</script>

{#if loader.error}
  <Notice kind="error">
    {loader.error}
    {#snippet action()}
      <Button variant="outline" size="sm" onclick={() => void loader.load()}>Retry</Button>
    {/snippet}
  </Notice>
{/if}
