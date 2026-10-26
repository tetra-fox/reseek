<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { dateTime } from "#lib/format.js";

  // where a value comes from; nothing for a default
  let { source }: { source: Schemas["OptionSource"] } = $props();
</script>

{#if source.layer === "env"}
  <Tooltip text="Set by an environment variable, which overrides the files" hint>
    {#snippet child({ props })}
      <Badge {...props} tone="info">{source.var}</Badge>
    {/snippet}
  </Tooltip>
{:else if source.layer === "cli"}
  <Tooltip text={`Set on the command line with ${source.flag}`} hint>
    {#snippet child({ props })}
      <Badge {...props} tone="info">cli</Badge>
    {/snippet}
  </Tooltip>
{:else if source.layer === "runtime"}
  <Tooltip text={`Applied at runtime by ${source.setBy}`} hint>
    {#snippet child({ props })}
      <Badge {...props} tone="accent">
        runtime{source.expiresAt ? ` until ${dateTime(source.expiresAt)}` : ""}
      </Badge>
    {/snippet}
  </Tooltip>
{:else if source.layer === "file"}
  <Badge tone="outline">{source.file}</Badge>
{/if}
