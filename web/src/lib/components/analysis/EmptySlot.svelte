<script lang="ts">
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
  import RecentResults from "./RecentResults.svelte";

  // a row with nothing in it: its letter in the corner and, centered, the ways to fill it, with the
  // panel of the daemon's files sliding open under them, a sheet on a phone. The first row keeps
  // the latest results there too, so the page is of use with nothing loaded
  type Props = { id: SlotId; onpicked: () => void };

  let { id, onpicked }: Props = $props();

  let picking = $state(false);
  let input = $state<HTMLInputElement | null>(null);
  const write = $derived(session.can("ReadWrite"));
  const opening = (node: Element) => slide(node, { duration: motionFast() });

  const pick = (target: Target, name: string) => {
    picking = false;
    slots.fill(id, target, name);
    onpicked();
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

<span class="absolute top-2 left-3 font-medium">{id.toUpperCase()}</span>

<div class="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 p-3">
  <EmptyState
    class="shrink-0 px-0 py-0"
    title={id === "a" ? "Add a file to analyze" : "Add a file to compare"}
    description={write && !viewport.phone
      ? "You can also drop a file anywhere in this row."
      : undefined}
    action={ways} />
  {#if picking && !viewport.phone}
    <div
      class="w-full max-w-2xl"
      transition:opening
      onintroend={(e) => e.currentTarget.scrollIntoView({ block: "nearest", behavior: "smooth" })}>
      <FilePicker onpick={pick} height="max(4rem, calc(var(--content-h) / 2 - 12rem))" />
    </div>
  {:else if id === "a"}
    <RecentResults />
  {/if}
</div>

{#if viewport.phone}
  <Dialog
    bind:open={picking}
    title="Add from Downloads"
    class="top-auto bottom-0 left-0 w-full max-w-none translate-x-0 translate-y-0 rounded-b-none">
    <div class="px-3 pb-3"><FilePicker onpick={pick} height="60svh" /></div>
  </Dialog>
{/if}
