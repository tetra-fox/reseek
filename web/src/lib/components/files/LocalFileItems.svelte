<script lang="ts">
  import type { Target } from "#lib/analysis/analysis.js";
  import AnalysisItems from "#lib/components/analysis/AnalysisItems.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";

  type Props = {
    /** opens the file in a new tab */
    onopen: () => void;
    /** the file as the analyzer names it, for a file the analyzer takes */
    analysis?: { target: Target; name: string } | null;
    /** saves it to this device */
    onsave: () => void;
    /** where it is on the daemon's machine, to copy */
    path?: string | null;
    /** names the path the local one, where the menu offers the remote one too */
    local?: boolean;
  };

  let { onopen, analysis, onsave, path, local = false }: Props = $props();

  const copy = $derived(local ? A.copyLocalPath : A.copyPath);
</script>

<!-- a file on the daemon's machine: open, analyze, save it, copy where it is -->
<Menu.Item onSelect={onopen}><A.open.icon />{A.open.label}</Menu.Item>
{#if analysis}<AnalysisItems {...analysis} />{/if}
<Menu.Item onSelect={onsave}><A.save.icon />{A.save.label}</Menu.Item>
{#if path}
  <Menu.Item onSelect={() => void copyText(path)}><copy.icon />{copy.label}</Menu.Item>
{/if}
