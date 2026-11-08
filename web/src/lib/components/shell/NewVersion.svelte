<script lang="ts">
  import MegaphoneIcon from "@lucide/svelte/icons/megaphone";

  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import { RELEASES } from "#lib/shell/project.js";

  const version = $derived(app.state?.version);
  let open = $state(false);
</script>

{#if version?.isUpdateAvailable}
  <Button variant="ghost" size="sm" class="text-warning-soft-fg" onclick={() => (open = true)}>
    <MegaphoneIcon class="text-warning" />
    <span class="max-sm:sr-only">Update available</span>
  </Button>
  <Dialog bind:open title="Update available">
    <p class="text-sm">
      reseek <strong class="whitespace-nowrap">{version.latest}</strong> is available. The current
      version is <strong class="whitespace-nowrap">{version.current}</strong>.
    </p>
    <a
      href={RELEASES}
      target="_blank"
      rel="noreferrer"
      class="inline-flex h-control w-full items-center justify-center rounded-control bg-accent text-sm font-medium text-accent-fg hover:bg-accent-hover">
      Release notes
    </a>
  </Dialog>
{/if}
