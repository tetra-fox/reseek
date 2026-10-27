<script lang="ts">
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";

  import type { Schemas } from "#lib/api/client.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { shortDirectory } from "#lib/dashboard/reports.js";

  // the directories peers took the most from us
  let { rows }: { rows: Schemas["DirectoryCount"][] } = $props();
</script>

<h3 class="flex items-center gap-1.5 text-sm font-semibold">
  <FolderOpenIcon class="size-4" />Directories
</h3>
<table class="w-full text-sm">
  <thead class="text-xs text-fg-muted">
    <tr class="border-b">
      <th class="w-8 py-1 text-right font-medium">#</th>
      <th class="px-2 py-1 text-left font-medium">Directory</th>
      <th class="px-2 py-1 text-right font-medium">
        <Tooltip text="times a file in this directory was uploaded to a peer">
          {#snippet child({ props })}<span {...props}>Served</span>{/snippet}
        </Tooltip>
      </th>
      <th class="px-2 py-1 text-right font-medium">Distinct Users</th>
    </tr>
  </thead>
  <tbody>
    {#each rows as row, i (row.directory)}
      <tr class="border-b last:border-b-0">
        <td class="py-1 text-right text-fg-muted tabular-nums">{i + 1}</td>
        <td class="px-2 py-1" title={row.directory}>{shortDirectory(row.directory)}</td>
        <td class="px-2 py-1 text-right tabular-nums">{row.count.toLocaleString()}</td>
        <td class="px-2 py-1 text-right tabular-nums">{row.distinctUsers.toLocaleString()}</td>
      </tr>
    {:else}
      <tr><td colspan="4" class="py-6 text-center text-fg-subtle">No data to display</td></tr>
    {/each}
  </tbody>
</table>
