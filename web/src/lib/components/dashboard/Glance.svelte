<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
  import MessageSquareIcon from "@lucide/svelte/icons/message-square";
  import RadioTowerIcon from "@lucide/svelte/icons/radio-tower";
  import type { Component } from "svelte";

  import { app } from "#lib/app.svelte.js";
  import { chat } from "#lib/chat/chat.svelte.js";
  import { formatCount, formatSpeed, plural } from "#lib/format.js";
  import { href } from "#lib/nav.js";
  import { SOULSEEK_SETTINGS } from "#lib/reachability.js";

  // what is happening now, by the daemon's live transfer figures as the footer shows them,
  // each figure a link to the list it counts

  /** a direction's active and queued transfers, with their speed */
  const now = (direction: "downloads" | "uploads") => {
    const m = app.metrics?.[direction];
    return {
      active: m?.inProgress.files ?? 0,
      queued: m?.queued.files ?? 0,
      speed: m?.inProgress.totalSpeed ?? 0
    };
  };
  const down = $derived(now("downloads"));
  const up = $derived(now("uploads"));
  const s = $derived(app.state);
  const reach = $derived(s?.server.listen.reachable);
</script>

{#snippet tile(
  Icon: Component,
  tone: string,
  title: string,
  parts: { text: string; to: string; strong?: boolean }[],
  note?: string
)}
  <div class="flex min-w-0 flex-col gap-1.5 p-4">
    <span class="flex items-center gap-1.5 text-xs text-fg-muted"
      ><Icon class={["size-3.5", tone]} />{title}</span>
    <span class="flex flex-wrap items-baseline gap-x-2 text-sm">
      {#each parts as p (p.text)}
        <a
          href={href(p.to)}
          class={[
            "underline-offset-2 hover:underline",
            p.strong ? "text-lg font-medium tabular-nums" : "text-fg-muted"
          ]}>{p.text}</a>
      {/each}
    </span>
    {#if note}<span class="truncate text-xs text-fg-muted">{note}</span>{/if}
  </div>
{/snippet}

<!-- an odd last tile takes the whole of its line, where two share each -->
<section
  aria-label="Current activity"
  class="tile-strip grid-cols-2 lg:grid-cols-5 [&>:last-child:nth-child(odd)]:max-lg:col-span-2">
  {@render tile(
    ArrowDownIcon,
    "text-download",
    "Downloading",
    [
      { text: `${formatCount(down.active)} active`, to: "/downloads?state=active", strong: true },
      { text: `${formatCount(down.queued)} queued`, to: "/downloads?state=queued" }
    ],
    down.active ? formatSpeed(down.speed) : undefined
  )}
  {@render tile(
    ArrowUpIcon,
    "text-upload",
    "Uploading",
    [
      { text: `${formatCount(up.active)} active`, to: "/uploads?state=active", strong: true },
      { text: `${formatCount(up.queued)} queued`, to: "/uploads?state=queued" }
    ],
    up.active ? formatSpeed(up.speed) : undefined
  )}
  {@render tile(MessageSquareIcon, chat.totalMentions ? "text-accent" : "text-fg-muted", "Chat", [
    { text: `${formatCount(chat.total)} unread`, to: "/chat", strong: true },
    { text: plural(chat.rooms.size, "room", "rooms"), to: "/chat" }
  ])}
  {@render tile(
    FolderTreeIcon,
    "text-fg-muted",
    "Sharing",
    [
      {
        text: plural(s?.shares.files ?? 0, "file", "files"),
        to: "/system/shares",
        strong: true
      },
      {
        text: `in ${plural(s?.shares.directories ?? 0, "folder", "folders")}`,
        to: "/system/shares"
      }
    ],
    s?.shares.scanning ? "Scanning" : s?.shares.scanPending ? "Rescan pending" : undefined
  )}
  {@render tile(
    RadioTowerIcon,
    reach === "no" ? "text-warning-icon" : reach === "yes" ? "text-success-icon" : "text-fg-muted",
    "Incoming connections",
    [
      {
        text: reach === "yes" ? "Reachable" : reach === "no" ? "Unreachable" : "Unknown",
        to: SOULSEEK_SETTINGS,
        strong: true
      }
    ],
    s ? `Port ${s.server.listen.port}` : undefined
  )}
</section>
