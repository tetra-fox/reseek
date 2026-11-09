<script lang="ts">
  import CopyIcon from "@lucide/svelte/icons/copy";

  import type { Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { formatBytes, formatCount } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";
  import { speedLimitsOf } from "#lib/transfers/speed.js";
  import { uploadsText } from "#lib/users/groups.js";

  import GroupsEditor from "./GroupsEditor.svelte";
  import NotesEditor from "./NotesEditor.svelte";
  import ProfileView from "./ProfileView.svelte";
  import UserInterests from "./UserInterests.svelte";

  // what the user says about themselves, what we note about them, and what we did together
  let { profile: p }: { profile: Schemas["UserProfile"] } = $props();

  /** the total upload limit in force, the alternative one while those are on */
  const uploadTotal = $derived.by(() => {
    const limits = speedLimitsOf(app.options, app.state);
    return (limits.alternative ? limits.alt : limits.normal).upload;
  });

  const write = $derived(session.can("ReadWrite"));
  const endpoint = $derived(p.endpoint ? `${p.endpoint.address}:${p.endpoint.port}` : null);
  const f = $derived(p.figures);
  const tiles = $derived([
    { label: "Downloads", value: f.downloads.succeeded, sub: formatBytes(f.downloads.bytes) },
    { label: "Uploads", value: f.uploads.succeeded, sub: formatBytes(f.uploads.bytes) },
    {
      label: "Failed",
      value: f.downloads.failed + f.uploads.failed,
      sub: `${formatCount(f.downloads.active + f.uploads.active)} active, ${formatCount(f.downloads.queued + f.uploads.queued)} queued`
    },
    {
      label: "Messages",
      value: f.messages.in + f.messages.out,
      sub: f.messages.unread
        ? `${formatCount(f.messages.unread)} unread`
        : `${formatCount(f.messages.in)} received`
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
    <p class="text-sm text-fg-muted">Use Refresh to load the profile</p>
  {/if}
</Card>

<UserInterests username={p.username} />

<div class="grid grid-cols-[minmax(0,1fr)] gap-section md:grid-cols-2">
  <Card title="Notes">
    <NotesEditor username={p.username} notes={p.notes} disabled={!write} />
  </Card>
  <Card title="Groups">
    <GroupsEditor username={p.username} groups={p.groups} disabled={!write} />
    <p class="mt-3 text-xs text-fg-muted">{uploadsText(p.uploads, uploadTotal)}</p>
    {#if endpoint}
      <p class="mt-4 flex items-center gap-1 text-sm">
        <span class="shrink-0 text-fg-muted">Address</span>
        <span class="min-w-0 font-mono wrap-anywhere">{endpoint}</span>
        <Tooltip text="Copy address">
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label="Copy address"
              onclick={() => void copyText(endpoint)}><CopyIcon /></Button>
          {/snippet}
        </Tooltip>
      </p>
    {/if}
  </Card>
</div>

<div class="tile-strip grid-cols-2 md:grid-cols-4">
  {#each tiles as tile (tile.label)}
    <div class="flex flex-col gap-0.5 p-4">
      <span class="text-xs text-fg-muted">{tile.label}</span>
      <span class="text-3xl font-semibold tracking-tight tabular-nums"
        >{formatCount(tile.value)}</span>
      <span class="text-xs text-fg-subtle">{tile.sub}</span>
    </div>
  {/each}
</div>
