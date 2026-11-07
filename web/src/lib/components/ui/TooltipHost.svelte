<script lang="ts" module>
  import { Tooltip } from "bits-ui";

  export type Side = "top" | "right" | "bottom" | "left";

  // the app has one tooltip, mounted once in the root layout. every Tooltip trigger tethers to it
  // and passes its text and side as the payload
  export const tether = Tooltip.createTether<{ text: string; side: Side }>();
</script>

<!-- bits-ui's default 700ms is slow for the collapsed sidebar, where the tooltip is the only label -->
<Tooltip.Provider delayDuration={300}>
  <!-- a click or a dialog moving focus opens no tooltip, so Escape reaches the dialog; keys still do -->
  <Tooltip.Root {tether} ignoreNonKeyboardFocus>
    {#snippet children({ payload })}
      <Tooltip.Portal>
        <Tooltip.Content
          side={payload?.side}
          sideOffset={4}
          collisionPadding={8}
          class="z-50 max-w-sm origin-(--bits-tooltip-content-transform-origin) animate-popover-in rounded-control border bg-overlay px-2 py-1 text-xs wrap-anywhere text-fg shadow-raised data-[state=closed]:animate-popover-out">
          {payload?.text}
        </Tooltip.Content>
      </Tooltip.Portal>
    {/snippet}
  </Tooltip.Root>
</Tooltip.Provider>
