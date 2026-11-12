<script lang="ts">
  import "../app.css";
  import { afterNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import { ModeWatcher } from "mode-watcher";

  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Toaster from "#lib/components/shell/Toaster.svelte";
  import TooltipHost from "#lib/components/ui/TooltipHost.svelte";
  import { followPointer } from "#lib/ui/hover.js";
  import { SCREEN_FAILED } from "#lib/wording.js";

  let { children } = $props();

  $effect(followPointer);

  /** the address that failed to show and what puts its page back, while the error stands in */
  let broken: { href: string; reset: () => void } | null = null;
  // a page that failed is tried again once the user goes to another address
  afterNavigate(({ to }) => {
    if (!broken || broken.href === to?.url.href) return;
    const { reset } = broken;
    broken = null;
    reset();
  });
</script>

<!-- the theme follows the OS until the user picks one, which is then kept in this browser -->
<ModeWatcher modeStorageKey="reseek-theme" defaultMode="system" />
<Toaster />
<TooltipHost />

<svelte:boundary onerror={(_, reset) => (broken = { href: page.url.href, reset })}>
  {@render children()}
  {#snippet failed(error)}
    <div class="flex min-h-svh items-center justify-center p-6">
      <!-- SvelteKit hands a boundary an App.Error, which hooks.client.ts gives the message -->
      <ErrorSegment caption={SCREEN_FAILED} detail={(error as App.Error).message} />
    </div>
  {/snippet}
</svelte:boundary>
