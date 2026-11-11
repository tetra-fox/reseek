<script lang="ts" module>
  import { Tooltip } from "bits-ui";

  import { safeArea } from "#lib/ui/safearea.svelte.js";

  export type Side = "top" | "right" | "bottom" | "left";

  // the app has one tooltip, mounted once in the root layout. every Tooltip trigger tethers to it
  // and passes its text and side as the payload
  export const tether = Tooltip.createTether<{ text: string; side: Side }>();
</script>

<!-- bits-ui's default 700ms is slow for the collapsed sidebar, where the tooltip is the only label.
     Only a tooltip that waited out the delay grows in: one opened just after another closed
     shows at once, so moving along a toolbar reads its labels without waiting -->
<Tooltip.Provider delayDuration={300}>
  <!-- a click or a dialog moving focus opens no tooltip, so Escape reaches the dialog; keys still do -->
  <Tooltip.Root {tether} ignoreNonKeyboardFocus>
    {#snippet children({ payload })}
      <Tooltip.Portal>
        <Tooltip.Content
          side={payload?.side}
          sideOffset={4}
          collisionPadding={safeArea.padding(8)}
          class="z-50 max-w-[min(24rem,calc(100vw-1rem))] origin-(--bits-tooltip-content-transform-origin) rounded-control border bg-overlay px-2 py-1 text-xs wrap-anywhere whitespace-pre-line text-fg shadow-raised data-[state=closed]:animate-popover-out data-[state=delayed-open]:animate-popover-in">
          {payload?.text}
        </Tooltip.Content>
      </Tooltip.Portal>
    {/snippet}
  </Tooltip.Root>
</Tooltip.Provider>
