<script lang="ts">
  import CopyIcon from "@lucide/svelte/icons/copy";

  import type { Schemas } from "#lib/api/client.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import { formatBytes } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";

  import NotesEditor from "./NotesEditor.svelte";
  import ProfileView from "./ProfileView.svelte";
  import TagsEditor from "./TagsEditor.svelte";

  // what the user says about themselves, what we note about them, and what we did together
  let { profile: p }: { profile: Schemas["UserProfile"] } = $props();

  const write = $derived(session.can("ReadWrite"));
  const endpoint = $derived(p.endpoint ? `${p.endpoint.address}:${p.endpoint.port}` : null);
  const f = $derived(p.figures);
  const tiles = $derived([
    { label: "Downloads", value: f.downloads.succeeded, sub: formatBytes(f.downloads.bytes) },
    { label: "Uploads", value: f.uploads.succeeded, sub: formatBytes(f.uploads.bytes) },
    {
      label: "Failed",
      value: f.downloads.failed + f.uploads.failed,
      sub: `${f.downloads.active + f.uploads.active} active, ${f.downloads.queued + f.uploads.queued} queued`
    },
    {
      label: "Messages",
      value: f.messages.in + f.messages.out,
      sub: f.messages.unread ? `${f.messages.unread} unread` : `${f.messages.in} received`
    }
  ]);
</script>

<Card title="Profile">
  {#if p.info}
    <ProfileView
      name={p.username}
      pictureUrl={p.info.pictureUrl}
      description={p.info.description}
      uploadSlots={p.info.uploadSlots}
      freeUploadSlot={p.info.hasFreeUploadSlot}
      queueLength={p.info.queueLength} />
  {:else}
    <p class="text-sm text-fg-muted">The profile has not been loaded. Use Refresh to request it.</p>
  {/if}
</Card>

<div class="grid gap-section md:grid-cols-2">
  <Card title="Notes">
    <NotesEditor username={p.username} notes={p.notes} disabled={!write} />
  </Card>
  <Card title="Tags">
    <TagsEditor username={p.username} tags={p.tags} disabled={!write} />
    {#if endpoint}
      <p class="mt-4 flex items-center gap-1 text-sm">
        <span class="text-fg-muted">Address</span>
        <span class="font-mono">{endpoint}</span>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Copy the address"
          onclick={() => void copyText(endpoint)}><CopyIcon /></Button>
      </p>
    {/if}
  </Card>
</div>

<div class="grid grid-cols-2 gap-section md:grid-cols-4">
  {#each tiles as tile (tile.label)}
    <div class="flex flex-col gap-0.5 rounded-card border bg-surface p-4 shadow-card">
      <span class="text-xs text-fg-muted">{tile.label}</span>
      <span class="text-2xl font-semibold tabular-nums">{tile.value.toLocaleString()}</span>
      <span class="text-xs text-fg-subtle">{tile.sub}</span>
    </div>
  {/each}
</div>
