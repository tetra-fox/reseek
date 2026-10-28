<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { fly } from "svelte/transition";

  import Button from "#lib/components/ui/Button.svelte";
  import { motionFast } from "#lib/motion.js";
  import { dismiss, toasts } from "#lib/toast.svelte.js";

  const motion = () => ({ duration: motionFast(), easing: cubicOut });
</script>

<!-- bottom centre, newest on top, above the footer -->
<div
  class="pointer-events-none fixed inset-x-4 bottom-[calc(var(--bottom-h)+1rem)] z-50 mx-auto flex max-w-md flex-col items-center gap-2"
  aria-live="polite">
  {#each toasts as t (t.id)}
    <div
      class={[
        "pointer-events-auto flex w-full items-center gap-2 rounded-card border bg-overlay py-2 pr-2 pl-4 text-sm shadow-raised",
        t.kind === "error" && "border-danger/50 bg-danger-soft text-danger-soft-fg"
      ]}
      role={t.kind === "error" ? "alert" : "status"}
      animate:flip={motion()}
      transition:fly={{ ...motion(), y: 8 }}>
      <span class="flex-1 wrap-anywhere">{t.text}</span>
      {#if t.action}
        {@const action = t.action}
        <Button
          size="sm"
          variant="outline"
          onclick={() => {
            dismiss(t.id);
            action.run();
          }}>{action.label}</Button>
      {/if}
      <Button variant="ghost" size="icon-sm" onclick={() => dismiss(t.id)} aria-label="Dismiss">
        <XIcon />
      </Button>
    </div>
  {/each}
</div>
