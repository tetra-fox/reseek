<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";

  import { CopyButton } from "#lib/clipboard.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";

  type Props = {
    caption: string;
    /** a second line under the caption */
    detail?: string;
    /** what the button does; reloads the page when not given */
    retry?: () => void;
    retryLabel?: string;
  };

  let { caption, detail, retry, retryLabel = "Reload" }: Props = $props();

  const copier = new CopyButton();
</script>

<!-- a failure that takes a whole region: what went wrong and a way to try again -->
<div class="flex flex-col items-center gap-3 px-6 py-10 text-center" role="alert">
  <span class="flex size-10 items-center justify-center rounded-full bg-danger-soft">
    <TriangleAlertIcon class="size-5 text-danger-soft-fg" />
  </span>
  <p class="text-sm font-medium wrap-anywhere">{caption}</p>
  {#if detail}<p class="max-w-sm text-sm text-pretty wrap-anywhere text-fg-muted">{detail}</p>{/if}
  <div class="flex gap-inline">
    <Button variant="outline" onclick={() => (retry ? retry() : location.reload())}>
      {retryLabel}
    </Button>
    <Button
      variant="ghost"
      onclick={() => void copier.copy([caption, detail].filter(Boolean).join("\n"))}>
      {#if copier.copied}<CheckIcon />Copied{:else}<CopyIcon />Copy the error{/if}
    </Button>
  </div>
</div>
