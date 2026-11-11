<script lang="ts">
  import { Popover } from "bits-ui";
  import type { Snippet } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import Switch from "#lib/components/ui/Switch.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import {
    formatLimit,
    speedLimitsOf,
    switchSpeedLimits,
    type LimitPair
  } from "#lib/transfers/speed.js";
  import { safeArea } from "#lib/ui/safearea.svelte.js";

  // the footer's speeds open the two pairs of speed limits and the switch between them
  type Props = { children: Snippet };
  let { children }: Props = $props();

  const limits = $derived(speedLimitsOf(app.options, app.state));
  const write = $derived(session.can("ReadWrite"));
  /** how long a switch the daemon took waits for the state frame to say so */
  const FRAME_WAIT_MS = 5_000;
  /** the choice sent and not yet back in the state frame */
  let sending = $state<boolean | null>(null);
  const alternative = $derived(sending ?? limits.alternative);
  // the switch shows the choice until the state frame carries it, so it never flicks back
  $effect(() => {
    if (sending !== null && limits.alternative === sending) sending = null;
  });

  const choose = async (on: boolean) => {
    sending = on;
    const outcome = await switchSpeedLimits(on);
    if (!outcome.ok) {
      toast(outcome.error, "error");
      sending = null;
      return;
    }
    // a frame that never comes, while the stream is down, leaves the state's answer shown
    setTimeout(() => {
      if (sending === on) sending = null;
    }, FRAME_WAIT_MS);
  };
</script>

{#snippet pair(title: string, values: LimitPair, inForce: boolean)}
  <div class={["flex flex-col gap-1 rounded-control px-2 py-1.5", inForce && "bg-surface-muted"]}>
    <span class="flex items-center justify-between gap-inline text-xs font-medium">
      {title}
      {#if inForce}<span class="text-fg-muted">In force</span>{/if}
    </span>
    <dl class="grid grid-cols-[auto_1fr] gap-x-3 text-xs">
      <dt class="text-fg-muted">Downloads</dt>
      <dd class="tabular-nums">{formatLimit(values.download)}</dd>
      <dt class="text-fg-muted">Uploads</dt>
      <dd class="tabular-nums">{formatLimit(values.upload)}</dd>
    </dl>
  </div>
{/snippet}

<Popover.Root>
  <Tooltip text="Speed limits">
    {#snippet child({ props })}
      <Popover.Trigger
        {...props}
        class="flex h-control-sm shrink-0 items-center gap-3 rounded-badge px-1 hover:bg-hover md:gap-4">
        <span class="sr-only">Speed limits:</span>
        {@render children()}
        {#if alternative}
          <span class="rounded-badge bg-warning-soft px-1.5 text-warning-soft-fg max-md:sr-only"
            >Alternative limits</span>
        {/if}
      </Popover.Trigger>
    {/snippet}
  </Tooltip>
  <Popover.Portal>
    <Popover.Content
      side="top"
      align="start"
      sideOffset={6}
      collisionPadding={safeArea.padding(8)}
      class="z-50 flex w-72 max-w-[calc(100vw-2rem)] flex-col gap-3 rounded-card border bg-overlay p-3 text-sm shadow-raised outline-none data-[state=closed]:animate-popover-out data-[state=open]:animate-popover-in">
      {#if write}
        <label class="flex items-center justify-between gap-inline">
          <span class="font-medium">Use alternative limits</span>
          <Switch
            checked={alternative}
            disabled={sending !== null}
            onCheckedChange={(on) => void choose(on)} />
        </label>
      {/if}
      <div class="flex flex-col gap-1">
        {@render pair("Normal limits", limits.normal, !alternative)}
        {@render pair("Alternative limits", limits.alt, alternative)}
      </div>
      {#if session.can("Administrator")}
        <a
          class="text-xs text-accent hover:underline"
          href={href("/system/settings?section=transfers")}>Change in Settings</a>
      {/if}
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>
