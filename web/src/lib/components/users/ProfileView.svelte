<script lang="ts">
  import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
  import CircleXIcon from "@lucide/svelte/icons/circle-x";

  import Linked from "#lib/components/Linked.svelte";

  import Avatar from "./Avatar.svelte";

  // a user's profile as a peer is sent it: the picture, the text and the upload figures. The
  // same view shows other users' profiles and the preview of our own
  type Props = {
    name: string;
    pictureUrl: string | null | undefined;
    /** a picture held in the page, shown instead of the saved one */
    src?: string | null;
    description: string;
    uploadSlots: number;
    freeUploadSlot: boolean;
    queueLength: number;
  };

  let { name, pictureUrl, src, description, uploadSlots, freeUploadSlot, queueLength }: Props =
    $props();
</script>

<div class="flex flex-col gap-4 sm:flex-row">
  <Avatar {name} {pictureUrl} {src} class="size-32 text-4xl" />
  <div class="flex min-w-0 flex-1 flex-col gap-3">
    {#if description}
      <p class="text-sm wrap-anywhere whitespace-pre-wrap"><Linked text={description} /></p>
    {:else}
      <p class="text-sm text-fg-muted">No profile text.</p>
    {/if}
    <dl class="flex flex-wrap gap-x-6 gap-y-1 text-xs">
      <div class="flex items-center gap-1">
        <dt class="text-fg-muted">Free upload slot</dt>
        <dd>
          {#if freeUploadSlot}
            <CircleCheckIcon class="size-3.5 text-success" aria-label="yes" />
          {:else}
            <CircleXIcon class="size-3.5 text-danger" aria-label="no" />
          {/if}
        </dd>
      </div>
      <div class="flex items-center gap-1">
        <dt class="text-fg-muted">Upload slots</dt>
        <dd class="tabular-nums">{uploadSlots}</dd>
      </div>
      <div class="flex items-center gap-1">
        <dt class="text-fg-muted">Queue length</dt>
        <dd class="tabular-nums">{queueLength}</dd>
      </div>
    </dl>
  </div>
</div>
