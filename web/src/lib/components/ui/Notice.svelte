<script lang="ts">
  import CircleXIcon from "@lucide/svelte/icons/circle-x";
  import InfoIcon from "@lucide/svelte/icons/info";
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
  import type { Snippet } from "svelte";

  type Props = {
    kind?: "info" | "warning" | "error";
    children: Snippet;
    action?: Snippet;
  };

  let { kind = "info", children, action }: Props = $props();

  const kinds = {
    info: { box: "border-border bg-surface-muted", tint: "text-fg-muted", Icon: InfoIcon },
    warning: {
      box: "border-warning/40 bg-warning-soft",
      tint: "text-warning-soft-fg",
      Icon: TriangleAlertIcon
    },
    error: {
      box: "border-danger/40 bg-danger-soft",
      tint: "text-danger-soft-fg",
      Icon: CircleXIcon
    }
  };
  const { box, tint, Icon } = $derived(kinds[kind]);
</script>

<div
  class={["flex items-start gap-2 rounded-control border px-3 py-2 text-sm text-fg", box]}
  role={kind === "error" ? "alert" : undefined}>
  <Icon class={["mt-0.5 size-4 shrink-0", tint]} />
  <div class="min-w-0 flex-1">{@render children()}</div>
  {#if action}
    <div class="-my-1 shrink-0">{@render action()}</div>
  {/if}
</div>
