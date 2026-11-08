<script lang="ts">
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";

  import type { Schemas } from "#lib/api/client.js";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { shortDirectory } from "#lib/dashboard/reports.js";

  // the directories peers took the most from us
  let { rows }: { rows: Schemas["DirectoryCount"][] } = $props();
</script>

<h3 class="flex items-center gap-1.5 text-sm font-medium">
  <FolderOpenIcon class="size-4" />Most uploaded folders
</h3>
<table class="data-table dense w-full text-sm">
  <thead>
    <tr>
      <th class="w-8 text-right">#</th>
      <th>Folder</th>
      <th class="text-right">Uploads</th>
      <th class="text-right">Users</th>
    </tr>
  </thead>
  <tbody>
    {#each rows as row, i (row.directory)}
      <tr>
        <td class="text-right text-fg-muted tabular-nums">{i + 1}</td>
        <Tooltip text={row.directory} dense>
          {#snippet child({ props })}
            <td {...props} class="font-mono text-xs wrap-anywhere whitespace-normal"
              >{shortDirectory(row.directory)}</td>
          {/snippet}
        </Tooltip>
        <td class="text-right tabular-nums">{row.count.toLocaleString()}</td>
        <td class="text-right tabular-nums">{row.distinctUsers.toLocaleString()}</td>
      </tr>
    {:else}
      <EmptyRow colspan={4} text="No uploads in this range" />
    {/each}
  </tbody>
</table>
