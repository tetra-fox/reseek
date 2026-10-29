<script lang="ts">
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import PauseIcon from "@lucide/svelte/icons/pause";
  import PlayIcon from "@lucide/svelte/icons/play";
  import SkipBackIcon from "@lucide/svelte/icons/skip-back";
  import SkipForwardIcon from "@lucide/svelte/icons/skip-forward";
  import Volume2Icon from "@lucide/svelte/icons/volume-2";
  import XIcon from "@lucide/svelte/icons/x";

  import Button from "#lib/components/ui/Button.svelte";
  import { player } from "#lib/files/player.svelte.js";
  import { formatSeconds, getFileName } from "#lib/format.js";
  import { href } from "#lib/nav.js";

  // the preview player; it lives in the shell, so it keeps playing while the user moves around
  let audio = $state<HTMLAudioElement | null>(null);
  let time = $state(0);
  let duration = $state(0);

  const track = $derived(player.current);

  // the element is the player's while it is on the page, so a view can follow and move it
  $effect(() => {
    player.element = audio;
    return () => (player.element = null);
  });
  $effect(() => {
    if (!audio || !player.src) return;
    if (player.playing) void audio.play().catch(() => (player.playing = false));
    else audio.pause();
  });
</script>

{#if track && player.src}
  <div
    class="relative z-20 flex flex-wrap items-center gap-inline border-t bg-surface/95 px-gutter py-2 shadow-raised backdrop-blur-md"
    role="region"
    aria-label="Player">
    <audio
      bind:this={audio}
      src={player.src}
      bind:currentTime={time}
      bind:duration
      bind:volume={player.volume}
      onloadedmetadata={() => {
        const at = player.takeStart();
        if (at > 0 && audio) audio.currentTime = at;
      }}
      onplay={() => (player.playing = true)}
      onpause={() => (player.playing = false)}
      onended={() => (player.index < player.tracks.length - 1 ? player.next() : null)}
      onerror={player.failed}></audio>
    <div class="flex items-center gap-1">
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Previous"
        disabled={player.index === 0}
        onclick={() => void player.previous()}>
        <SkipBackIcon />
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={player.playing ? "Pause" : "Play"}
        onclick={() => (player.playing = !player.playing)}>
        {#if player.playing}<PauseIcon />{:else}<PlayIcon />{/if}
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Next"
        disabled={player.index >= player.tracks.length - 1}
        onclick={() => void player.next()}>
        <SkipForwardIcon />
      </Button>
    </div>
    <span class="min-w-0 flex-1 truncate text-sm" title={track.filename}>
      {getFileName(track.filename)}
    </span>
    <span class="text-xs text-fg-muted tabular-nums">
      {formatSeconds(time)} / {formatSeconds(duration || 0)}
    </span>
    <input
      type="range"
      aria-label="Position"
      min="0"
      max={duration || 0}
      step="1"
      bind:value={time}
      class="w-40 accent-(--color-accent)" />
    <span class="flex items-center gap-1">
      <Volume2Icon class="size-4 text-fg-muted" />
      <input
        type="range"
        aria-label="Volume"
        min="0"
        max="1"
        step="0.05"
        bind:value={player.volume}
        class="w-20 accent-(--color-accent)" />
    </span>
    {#if track.folder}
      <Button
        variant="ghost"
        size="sm"
        href={href(
          `/system/files?root=${track.folder.root}&path=${encodeURIComponent(track.folder.path)}`
        )}>
        <FolderOpenIcon />Open folder
      </Button>
    {/if}
    <Button variant="ghost" size="icon-sm" aria-label="Close the player" onclick={player.close}>
      <XIcon />
    </Button>
  </div>
{/if}
