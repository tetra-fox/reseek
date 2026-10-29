<script lang="ts">
  import UserIcon from "@lucide/svelte/icons/user";

  import { JUDGMENTS } from "#lib/analysis/analysis.js";
  import { type Mark, markHint } from "#lib/analysis/marks.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // a file's analysis in a list: the user's judgment with a person beside it, or else a muted
  // "Analyzed" whose tooltip gives the analyzer's reading, never in a warning color of its own
  let { mark }: { mark: Mark | undefined } = $props();
</script>

{#if mark}
  <Tooltip text={markHint(mark)} hint>
    {#snippet child({ props })}
      {#if mark.judgment}
        <Badge {...props} tone={mark.judgment === "fake" ? "danger" : "success"}
          ><UserIcon aria-hidden="true" />{JUDGMENTS[mark.judgment]}</Badge>
      {:else}
        <Badge {...props}>Analyzed</Badge>
      {/if}
    {/snippet}
  </Tooltip>
{/if}
