<script lang="ts">
  import "../app.css";
  import { ModeWatcher } from "mode-watcher";

  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Toaster from "#lib/components/shell/Toaster.svelte";
  import TooltipHost from "#lib/components/ui/TooltipHost.svelte";
  import { SCREEN_FAILED } from "#lib/wording.js";

  let { children } = $props();
</script>

<!-- the theme follows the OS until the user picks one, which is then kept in this browser -->
<ModeWatcher modeStorageKey="reseek-theme" defaultMode="system" />
<Toaster />
<TooltipHost />

<svelte:boundary>
  {@render children()}
  {#snippet failed(error)}
    <div class="flex min-h-svh items-center justify-center p-6">
      <!-- SvelteKit hands a boundary an App.Error, which hooks.client.ts gives the message -->
      <ErrorSegment caption={SCREEN_FAILED} detail={(error as App.Error).message} />
    </div>
  {/snippet}
</svelte:boundary>
