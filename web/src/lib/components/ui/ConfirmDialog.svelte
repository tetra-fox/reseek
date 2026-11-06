<script lang="ts">
  import { AlertDialog } from "bits-ui";

  import Button from "#lib/components/ui/Button.svelte";

  type Props = {
    open: boolean;
    title: string;
    body?: string;
    /** the confirm button's words */
    confirm: string;
    /** the other button's words, Cancel by default */
    dismiss?: string;
    danger?: boolean;
    onconfirm: () => void;
    /** where focus goes once it closes; the default returns it to what opened the dialog */
    onclosefocus?: (e: Event) => void;
  };

  let {
    open = $bindable(),
    title,
    body,
    confirm,
    dismiss = "Cancel",
    danger = false,
    onconfirm,
    onclosefocus
  }: Props = $props();
</script>

<!-- an alert dialog, so Esc and the overlay cancel and never confirm -->
<AlertDialog.Root bind:open>
  <AlertDialog.Portal>
    <AlertDialog.Overlay
      class="fixed inset-0 z-50 bg-scrim data-[state=closed]:animate-fade-out data-[state=open]:animate-fade-in" />
    <AlertDialog.Content
      onCloseAutoFocus={onclosefocus}
      class="fixed top-1/2 left-1/2 z-50 flex w-[min(26rem,calc(100vw-2rem))] -translate-x-1/2 -translate-y-1/2 flex-col gap-4 rounded-card border bg-surface p-5 shadow-raised outline-none data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
      <div class="flex flex-col gap-1">
        <AlertDialog.Title class="text-base font-medium">{title}</AlertDialog.Title>
        {#if body}
          <AlertDialog.Description class="text-sm text-fg-muted">{body}</AlertDialog.Description>
        {/if}
      </div>
      <div class="flex justify-end gap-inline">
        <AlertDialog.Cancel>
          {#snippet child({ props })}
            <Button {...props} variant="ghost">{dismiss}</Button>
          {/snippet}
        </AlertDialog.Cancel>
        <AlertDialog.Action>
          {#snippet child({ props })}
            <Button
              {...props}
              variant={danger ? "danger" : "primary"}
              onclick={() => {
                open = false;
                onconfirm();
              }}>{confirm}</Button>
          {/snippet}
        </AlertDialog.Action>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>
