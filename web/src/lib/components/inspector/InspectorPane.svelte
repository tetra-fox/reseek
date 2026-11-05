<script lang="ts">
  import XIcon from "@lucide/svelte/icons/x";
  import { Dialog } from "bits-ui";
  import { fly, slide } from "svelte/transition";

  import Button from "#lib/components/ui/Button.svelte";
  import ResizeHandle from "#lib/components/ui/ResizeHandle.svelte";
  import {
    inspector,
    inspectorPrefs,
    PANE_MIN,
    PANE_WIDTH,
    paneMax
  } from "#lib/inspector/inspector.svelte.js";
  import { motionFast } from "#lib/ui/motion.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  /** the docked width: the remembered one, or less in a window too narrow for it */
  let width = $state(Math.min(inspectorPrefs.width, paneMax()));
  /** docked it opens and closes by its width, which moves the content with it */
  const opening = (node: Element) => slide(node, { axis: "x", duration: motionFast() });
  const sheetOpening = (node: Element) => fly(node, { y: 24, duration: motionFast() });
</script>

{#snippet body()}
  {#if inspector.present()}
    {@render inspector.content?.()}
  {:else}
    <div class="flex items-center justify-end p-2">
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Close details"
        onclick={() => inspector.close()}><XIcon /></Button>
    </div>
    <p class="px-4 text-sm text-fg-muted">Select a file to see its details.</p>
  {/if}
{/snippet}

<!-- on a wider screen a pane beside the content area, whose list narrows to make room; a phone
     has no room to spare for one, so there it is a sheet over the whole screen. The slide is
     local: a pane open when the app starts is just there -->
{#if viewport.phone}
  <Dialog.Root bind:open={() => inspector.open, (open) => (inspector.sheet = open)}>
    <Dialog.Portal>
      <Dialog.Content forceMount aria-label="Details">
        {#snippet child({ props, open })}
          {#if open}
            <div
              {...props}
              class="fixed inset-0 z-50 flex flex-col bg-surface outline-none"
              data-inspector
              transition:sheetOpening>
              {@render body()}
            </div>
          {/if}
        {/snippet}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{:else if inspector.open}
  <aside
    aria-label="Details"
    class="relative flex max-w-[50vw] shrink-0 flex-col border-l bg-surface"
    style:width={`${width}px`}
    data-inspector
    tabindex="-1"
    {@attach inspector.attachPane}
    transition:opening>
    <ResizeHandle
      bind:value={width}
      min={PANE_MIN}
      max={paneMax()}
      initial={PANE_WIDTH}
      edge="left"
      label="Width of the details pane"
      onchange={(w) => (inspectorPrefs.width = w)}
      class="absolute inset-y-0 -left-1 z-10 w-2" />
    {@render body()}
  </aside>
{/if}
