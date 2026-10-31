<script lang="ts">
  import Trash2Icon from "@lucide/svelte/icons/trash-2";

  import { targetKey } from "#lib/analysis/analysis.js";
  import { analyzable, marks } from "#lib/analysis/marks.svelte.js";
  import AnalysisMark from "#lib/components/analysis/AnalysisMark.svelte";
  import LocalFileItems from "#lib/components/files/LocalFileItems.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { isAudio } from "#lib/files/kinds.js";
  import { dateTime, formatBytes } from "#lib/format.js";
  import { inspector } from "#lib/inspector/inspector.svelte.js";
  import type { Entry } from "#lib/system/explorer.js";
  import { fileTrack, type Root } from "#lib/system/localfiles.js";
  import { shiftHeld } from "#lib/ui/keys.js";

  import Inspector from "./Inspector.svelte";

  // a file under the daemon's download directories, as System > Files lists it
  type Props = {
    entry: Entry;
    root: Root;
    onopen: () => void;
    /** plays it in the page's player */
    onpreview: () => void;
    onsave: () => void;
    /** asks to delete it, where files can be deleted */
    ondelete?: () => void;
  };

  let { entry, root, onopen, onpreview, onsave, ondelete }: Props = $props();

  const audio = $derived(isAudio(entry.name));
  const mark = $derived(marks.file(root, entry.path));
  const analysis = $derived(
    analyzable(entry.name)
      ? {
          target: { kind: "file" as const, root, path: entry.path },
          track: fileTrack(root, entry.path)
        }
      : null
  );
  const folder = $derived(entry.path.split("/").slice(0, -1).join("/"));
</script>

{#snippet chips()}
  <AnalysisMark {mark} />
{/snippet}

{#snippet actions()}
  <Button variant="outline" size="sm" onclick={onopen}><A.open.icon />{A.open.label}</Button>
  {#if audio}
    <Button variant="outline" size="sm" onclick={onpreview}
      ><A.preview.icon />{A.preview.label}</Button>
  {/if}
{/snippet}

{#snippet menu()}
  <Menu.Content align="end" class="min-w-48">
    <LocalFileItems
      {onopen}
      onpreview={audio ? onpreview : undefined}
      onanalyze={analysis
        ? () => inspector.analyze(targetKey(analysis.target), shiftHeld())
        : undefined}
      {onsave}
      path={entry.path} />
    {#if ondelete}
      <Menu.Separator />
      <Menu.Item onSelect={ondelete}><Trash2Icon />Delete</Menu.Item>
    {/if}
  </Menu.Content>
{/snippet}

<Inspector
  name={entry.name}
  path={entry.path}
  chips={mark ? chips : undefined}
  size={entry.size ? formatBytes(entry.size) : null}
  {actions}
  {menu}
  details={[
    { label: "Folder", value: folder },
    { label: "Modified", value: entry.modifiedAt ? dateTime(entry.modifiedAt) : null }
  ]}
  {analysis}
  technical={[{ label: "Path", value: entry.path }]} />
