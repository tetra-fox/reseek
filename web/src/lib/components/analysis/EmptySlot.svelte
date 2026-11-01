<script lang="ts">
  import AudioLinesIcon from "@lucide/svelte/icons/audio-lines";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { slide } from "svelte/transition";

  import type { SlotId } from "#lib/analysis/address.js";
  import type { Target } from "#lib/analysis/analysis.js";
  import { analyzedTypes } from "#lib/analysis/marks.svelte.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import { session } from "#lib/session.svelte.js";
  import { motionFast } from "#lib/ui/motion.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  import FilePicker from "./FilePicker.svelte";

  // a slot with nothing in it: the ways to fill it, full size or as a slim bar, with the panel of
  // the daemon's files sliding open under them, a sheet on a phone
  type Props = { id: SlotId; slim: boolean };

  let { id, slim }: Props = $props();

  let picking = $state(false);
  let input = $state<HTMLInputElement | null>(null);
  const write = $derived(session.can("ReadWrite"));
  const opening = (node: Element) => slide(node, { duration: motionFast() });

  const pick = (target: Target, name: string) => {
    picking = false;
    slots.fill(id, target, name);
  };
  const chosen = () => {
    const file = input?.files?.[0];
    if (file) slots.upload(id, file);
  };
</script>

{#snippet ways()}
  <div class="flex flex-wrap items-center justify-center gap-inline">
    <Button variant="outline" size="sm" aria-expanded={picking} onclick={() => (picking = !picking)}
      ><FolderOpenIcon />Add from Downloads</Button>
    {#if write}
      <Button variant="outline" size="sm" onclick={() => input?.click()}
        ><UploadIcon />Upload file</Button>
      <input
        bind:this={input}
        type="file"
        class="hidden"
        accept={analyzedTypes()}
        aria-label={`Upload a file to slot ${id.toUpperCase()}`}
        onchange={chosen} />
    {/if}
  </div>
{/snippet}

{#if slim}
  <div class="flex flex-wrap items-center gap-inline px-3 py-2">
    <span class="font-medium">{id.toUpperCase()}</span>
    <p class="min-w-0 flex-1 text-sm text-fg-muted">Add a file to compare, or drop one here.</p>
    {@render ways()}
  </div>
{:else}
  <EmptyState
    icon={AudioLinesIcon}
    title="Add a file to analyze"
    description="Choose a file from the daemon or from this device, or drop one here."
    action={ways} />
{/if}

{#if viewport.phone}
  <Dialog
    bind:open={picking}
    title="Add from Downloads"
    class="top-auto bottom-0 left-0 w-full max-w-none translate-x-0 translate-y-0 rounded-b-none">
    <div class="px-3 pb-3"><FilePicker onpick={pick} height="60svh" /></div>
  </Dialog>
{:else if picking}
  <div
    class="border-t p-3"
    transition:opening
    onintroend={(e) => e.currentTarget.scrollIntoView({ block: "nearest", behavior: "smooth" })}>
    <FilePicker onpick={pick} height="min(24rem, 50svh)" />
  </div>
{/if}
