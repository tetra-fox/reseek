<script lang="ts">
  import CircleHelpIcon from "@lucide/svelte/icons/circle-help";
  import { Popover } from "bits-ui";

  import Button from "#lib/components/ui/Button.svelte";
  import { FILTER_TOKENS } from "#lib/files/filters.js";
  import { safeArea } from "#lib/ui/safearea.svelte.js";
</script>

<!-- every token the filter box takes, one click away from it -->
<Popover.Root>
  <Popover.Trigger>
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="icon-sm" aria-label="Filter syntax">
        <CircleHelpIcon />
      </Button>
    {/snippet}
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content
      align="end"
      sideOffset={6}
      collisionPadding={safeArea.padding(8)}
      class="z-50 max-h-(--bits-popover-content-available-height) w-[min(30rem,calc(100vw-2rem))] overflow-y-auto rounded-card border bg-overlay p-4 text-sm shadow-raised outline-none data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
      <p class="mb-2 text-fg-muted">
        Every word must appear in the path. A file with no bitrate or other attribute passes that
        attribute's filter.
      </p>
      <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
        {#each FILTER_TOKENS as t (t.token)}
          <dt><code class="text-xs">{t.token}</code></dt>
          <dd class="text-fg-muted">{t.meaning}</dd>
        {/each}
      </dl>
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>
