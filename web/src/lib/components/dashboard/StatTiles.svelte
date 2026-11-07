<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import ChartPieIcon from "@lucide/svelte/icons/chart-pie";
  import UserIcon from "@lucide/svelte/icons/user";
  import type { Component } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import { ratioTone, tiles } from "#lib/dashboard/reports.js";
  import { formatBytes, NONE, plural } from "#lib/format.js";
  import { href } from "#lib/nav.js";

  // what the range moved: bytes and files each way, the share ratio and the peers
  let { summary }: { summary: Schemas["SummaryReport"] } = $props();

  const t = $derived(tiles(summary));
  const tone = $derived(ratioTone(t.ratio));
  /** a size as its number and its unit, to one decimal */
  const split = (bytes: number) => formatBytes(bytes, 1).split(" ") as [string, string];
  const down = $derived(split(t.downloaded));
  const up = $derived(split(t.uploaded));
  const TONES = {
    muted: "text-fg-muted",
    success: "text-success",
    warning: "text-warning",
    danger: "text-danger"
  };
</script>

{#snippet tile(
  Icon: Component,
  stat: {
    icon?: string;
    label: string;
    value: string;
    unit?: string;
    tone?: string;
    note?: string;
  },
  to: string
)}
  <a
    href={href(to)}
    class="group flex flex-col gap-1.5 p-4 transition-colors duration-(--motion-fast) hover:bg-hover">
    <span class="flex items-center gap-1.5 text-xs text-fg-muted group-hover:text-fg"
      ><Icon class={["size-3.5", stat.icon]} />{stat.label}</span>
    <span
      class={[
        "flex items-baseline gap-1 text-3xl font-semibold tracking-tight tabular-nums",
        stat.tone
      ]}
      >{stat.value}{#if stat.unit}<span class="text-sm text-fg-muted">{stat.unit}</span>{/if}</span>
    {#if stat.note}<span class="truncate text-xs text-fg-subtle">{stat.note}</span>{/if}
  </a>
{/snippet}

<div class="tile-strip grid-cols-2 lg:grid-cols-4">
  {@render tile(
    ArrowDownIcon,
    {
      icon: "text-download",
      tone: "text-download",
      label: "Downloaded",
      value: down[0],
      unit: down[1],
      note: plural(t.downloadedFiles, "file", "files")
    },
    "/downloads?state=completed"
  )}
  {@render tile(
    ArrowUpIcon,
    {
      icon: "text-upload",
      tone: "text-upload",
      label: "Uploaded",
      value: up[0],
      unit: up[1],
      note: plural(t.uploadedFiles, "file", "files")
    },
    "/uploads?state=completed"
  )}
  {@render tile(
    ChartPieIcon,
    {
      label: "Share ratio",
      value: t.ratio === null ? NONE : t.ratio.toFixed(2),
      tone: TONES[tone],
      note: "Uploaded / downloaded"
    },
    "/uploads"
  )}
  {@render tile(
    UserIcon,
    { label: "Users transferred with", value: t.peers.toLocaleString() },
    "/users?has=transfers"
  )}
</div>
