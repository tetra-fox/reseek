<script lang="ts">
  import UserIcon from "@lucide/svelte/icons/user";

  import { JUDGMENTS } from "#lib/analysis/analysis.js";
  import { type Mark, markHint } from "#lib/analysis/marks.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // a file's analysis in a list: the user's judgment with a person beside it, or else a muted
  // "Analyzed" whose tooltip gives the analyzer's reading, never in a warning color of its own
  let { mark }: { mark: Mark | undefined } = $props();

  // the tooltip reads these lazily, and can do so after `mark` is gone but before the block below
  // is torn down, so they are worked out here and not from `mark` inside it
  const hint = $derived(mark && markHint(mark));
  const judgment = $derived(mark?.judgment);
</script>

{#if mark}
  <Tooltip text={hint} dense spoken>
    {#snippet child({ props })}
      {#if judgment}
        <Badge {...props} tone={judgment === "fake" ? "danger" : "success"}
          ><UserIcon aria-hidden="true" />{JUDGMENTS[judgment]}</Badge>
      {:else}
        <Badge {...props}>Analyzed</Badge>
      {/if}
    {/snippet}
  </Tooltip>
{/if}
