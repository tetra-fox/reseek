<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";
  import { Dialog } from "bits-ui";
  import type { Snippet } from "svelte";

  import Button from "#lib/components/ui/Button.svelte";
  import Scrim from "#lib/components/ui/Scrim.svelte";
  import { cn } from "#lib/ui/cn.js";

  type Props = {
    open: boolean;
    title: string;
    description?: string;
    /** under the body, for the dialog's buttons */
    footer?: Snippet;
    /**
     * a request the dialog made is running: Esc, the overlay and the close button leave it open
     * until it ends, so what the request does next happens in the dialog that asked for it
     */
    busy?: boolean;
    /** where focus goes when it closes, in place of what had it before it opened */
    returnTo?: () => HTMLElement | null;
    class?: string;
    children: Snippet;
  };

  let {
    open = $bindable(),
    title,
    description,
    footer,
    busy = false,
    returnTo,
    class: className,
    children
  }: Props = $props();
</script>

<!-- a centered box over the page, closed by Esc, the overlay or its close button unless busy -->
<Dialog.Root bind:open>
  <Dialog.Portal>
    <Scrim />
    <Dialog.Content
      escapeKeydownBehavior={busy ? "ignore" : "close"}
      interactOutsideBehavior={busy ? "ignore" : "close"}
      onCloseAutoFocus={(e) => {
        const to = returnTo?.();
        if (!to?.isConnected) return;
        e.preventDefault();
        to.focus({ preventScroll: true });
      }}
      class={cn(
        "fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100svh-var(--safe-t)-var(--safe-b)-2rem)] w-[min(28rem,calc(100vw-2rem))] max-w-(--safe-w) -translate-x-1/2 -translate-y-1/2 flex-col rounded-card border bg-surface shadow-raised outline-none data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in",
        className
      )}>
      <div class="flex flex-col gap-1 py-4 pr-14 pl-5">
        <Dialog.Title class="text-base font-medium wrap-anywhere">{title}</Dialog.Title>
        {#if description}
          <Dialog.Description class="text-sm text-fg-muted">{description}</Dialog.Description>
        {/if}
      </div>
      <div class="flex min-h-0 flex-1 flex-col gap-group overflow-y-auto px-5 pb-4">
        {@render children()}
      </div>
      {#if footer}
        <div class="flex flex-wrap items-center justify-end gap-inline border-t px-5 py-3">
          {@render footer()}
        </div>
      {/if}
      <Dialog.Close>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon-sm"
            class="absolute top-3.5 right-3.5"
            disabled={busy}
            aria-label="Close">
            <XIcon />
          </Button>
        {/snippet}
      </Dialog.Close>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
