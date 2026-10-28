<script lang="ts">
  import UserIcon from "@lucide/svelte/icons/user";

  import { JUDGEMENTS } from "#lib/analysis/analysis.js";
  import { type Mark, markHint } from "#lib/analysis/marks.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // a file's analysis in a list: the user's judgement with a person beside it, or else a muted
  // "Analysed" whose tooltip gives the analyser's reading, never in a warning colour of its own
  let { mark }: { mark: Mark | undefined } = $props();
</script>

{#if mark}
  <Tooltip text={markHint(mark)} hint>
    {#snippet child({ props })}
      {#if mark.judgement}
        <Badge {...props} tone={mark.judgement === "fake" ? "danger" : "success"}
          ><UserIcon aria-hidden="true" />{JUDGEMENTS[mark.judgement]}</Badge>
      {:else}
        <Badge {...props}>Analysed</Badge>
      {/if}
    {/snippet}
  </Tooltip>
{/if}
