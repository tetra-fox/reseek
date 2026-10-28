<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import ChartPieIcon from "@lucide/svelte/icons/chart-pie";
  import UserIcon from "@lucide/svelte/icons/user";
  import type { Component } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import { ratioTone, tiles } from "#lib/dashboard/reports.js";
  import { formatBytes } from "#lib/format.js";
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
  value: string,
  unit: string,
  label: string,
  color: string,
  to: string
)}
  <a
    href={href(to)}
    class="group flex flex-col gap-1 rounded-card border bg-surface p-4 shadow-card transition-colors duration-(--motion-fast) hover:border-fg-subtle/40">
    <span class={["flex items-baseline gap-1 text-2xl font-semibold tabular-nums", color]}>
      <Icon class="size-4 self-center" />{value}<span class="text-sm font-normal">{unit}</span>
    </span>
    <span class="text-xs text-fg-muted group-hover:text-fg">{label}</span>
  </a>
{/snippet}

<div class="grid grid-cols-2 gap-section lg:grid-cols-4">
  {@render tile(
    ArrowDownIcon,
    down[0],
    down[1],
    `Downloaded · ${t.downloadedFiles.toLocaleString()} files`,
    "text-info",
    "/downloads?state=completed"
  )}
  {@render tile(
    ArrowUpIcon,
    up[0],
    up[1],
    `Uploaded · ${t.uploadedFiles.toLocaleString()} files`,
    "text-success",
    "/uploads?state=completed"
  )}
  {@render tile(
    ChartPieIcon,
    t.ratio === null ? "—" : t.ratio.toFixed(2),
    "",
    "Share ratio (↑/↓)",
    TONES[tone],
    "/uploads"
  )}
  {@render tile(
    UserIcon,
    t.peers.toLocaleString(),
    "",
    "Users transferred with",
    "",
    "/users?has=transfers"
  )}
</div>
