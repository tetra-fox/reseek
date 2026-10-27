<script lang="ts">
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import { confirmation } from "#lib/confirm.svelte.js";

  // the dialog behind `confirmation.ask`, mounted once by the app shell
  const q = $derived(confirmation.question);
</script>

{#if q}
  <ConfirmDialog
    bind:open={confirmation.open}
    title={q.title}
    body={q.body}
    confirm={q.confirm}
    danger
    onconfirm={q.run} />
{/if}
