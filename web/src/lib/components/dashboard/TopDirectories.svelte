<script lang="ts">
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";

  import type { Schemas } from "#lib/api/client.js";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import { shortDirectory } from "#lib/dashboard/reports.js";

  // the directories peers took the most from us
  let { rows }: { rows: Schemas["DirectoryCount"][] } = $props();
</script>

<h3 class="flex items-center gap-1.5 text-sm font-medium">
  <FolderOpenIcon class="size-4" />Most uploaded folders
</h3>
<table class="w-full text-sm">
  <thead class="text-xs text-fg-muted">
    <tr class="border-b">
      <th class="w-8 py-1 text-right font-medium">#</th>
      <th class="px-2 py-1 text-left font-medium">Folder</th>
      <th class="px-2 py-1 text-right font-medium"> Uploads </th>
      <th class="px-2 py-1 text-right font-medium">Users</th>
    </tr>
  </thead>
  <tbody>
    {#each rows as row, i (row.directory)}
      <tr class="border-b last:border-b-0">
        <td class="py-1 text-right text-fg-muted tabular-nums">{i + 1}</td>
        <td class="px-2 py-1 wrap-anywhere" title={row.directory}
          >{shortDirectory(row.directory)}</td>
        <td class="px-2 py-1 text-right tabular-nums">{row.count.toLocaleString()}</td>
        <td class="px-2 py-1 text-right tabular-nums">{row.distinctUsers.toLocaleString()}</td>
      </tr>
    {:else}
      <EmptyRow colspan={4} text="No uploads in this range" />
    {/each}
  </tbody>
</table>
