<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { fly } from "svelte/transition";

  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { dismiss, hold, toasts } from "#lib/feedback/toast.svelte.js";
  import { motionFast } from "#lib/ui/motion.js";

  const motion = () => ({ duration: motionFast(), easing: cubicOut });
</script>

<!-- bottom center, newest on top, above the footer and the docked activity log -->
<div
  class="pointer-events-none fixed inset-x-4 bottom-[calc(var(--bottom-h)+1rem)] z-50 mx-auto flex max-w-md flex-col items-center gap-2"
  aria-live="polite">
  {#each toasts as t (t.id)}
    <div
      class={[
        "pointer-events-auto relative flex w-full items-center gap-2 overflow-hidden rounded-card border bg-overlay py-2 pr-2 pl-4 text-sm shadow-raised",
        t.kind === "error" && "border-danger/50 bg-danger-soft text-danger-soft-fg"
      ]}
      role={t.kind === "error" ? "alert" : "status"}
      onpointerenter={() => hold(t.id, "pointer", true)}
      onpointerleave={() => hold(t.id, "pointer", false)}
      onfocusin={() => hold(t.id, "focus", true)}
      onfocusout={(e) => {
        // focus moving between the toast's own buttons keeps it held
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) hold(t.id, "focus", false);
      }}
      animate:flip={motion()}
      transition:fly={{ ...motion(), y: 8 }}>
      <span class="flex-1 wrap-anywhere">{t.text}</span>
      {#if t.action}
        {@const action = t.action}
        <Button
          size="sm"
          variant="outline"
          onclick={() => {
            action.run();
            dismiss(t.id);
          }}>{action.label}</Button>
      {/if}
      <Tooltip text="Dismiss">
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon-sm"
            onclick={() => dismiss(t.id)}
            aria-label="Dismiss">
            <XIcon />
          </Button>
        {/snippet}
      </Tooltip>
      {#if Number.isFinite(t.ms)}
        <!-- the time left, run from the toast's own duration and held with it, from the start
        again when another toast takes its place; none under reduced motion, where a bar that
        does not move says nothing -->
        {#key t.round}
          <div
            class={[
              "countdown absolute inset-x-0 bottom-0 h-0.5 origin-left motion-reduce:hidden",
              t.kind === "error" ? "bg-danger" : "bg-accent"
            ]}
            style:animation-duration="{t.ms}ms"
            style:animation-play-state={t.held ? "paused" : "running"}
            aria-hidden="true">
          </div>
        {/key}
      {/if}
    </div>
  {/each}
</div>

<style>
  .countdown {
    animation-name: countdown;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }

  @keyframes countdown {
    to {
      transform: scaleX(0);
    }
  }
</style>
