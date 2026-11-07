<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { lasting } from "#lib/config/editability.js";

  // where a value comes from; nothing for a default
  let { source }: { source: Schemas["OptionSource"] } = $props();
</script>

{#if source.layer === "env"}
  <Tooltip text="Set by an environment variable, which overrides the files" dense spoken>
    {#snippet child({ props })}
      <Badge {...props} tone="info">{source.var}</Badge>
    {/snippet}
  </Tooltip>
{:else if source.layer === "cli"}
  <Tooltip text={`Set on the command line with ${source.flag}`} dense spoken>
    {#snippet child({ props })}
      <Badge {...props} tone="info">cli</Badge>
    {/snippet}
  </Tooltip>
{:else if source.layer === "runtime"}
  <Tooltip text={`Applied at runtime by ${source.setBy}`} dense spoken>
    {#snippet child({ props })}
      <Badge {...props} tone="accent">
        runtime {lasting(source)}
      </Badge>
    {/snippet}
  </Tooltip>
{:else if source.layer === "file"}
  <Badge tone="outline">{source.file}</Badge>
{/if}
