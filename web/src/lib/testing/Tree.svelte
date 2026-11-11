<script lang="ts">
  import type { SvelteSet } from "svelte/reactivity";

  import FileTree from "#lib/components/files/FileTree.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import SelectAll from "#lib/components/ui/SelectAll.svelte";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { folderNode, treeLines } from "#lib/tree/folders.js";

  type Item = { id: string };

  // a tree of three folders for the shared tree's tests: A holding a1, a2 and the folder B with
  // b1, and C holding c1, every folder closed
  let {
    selected,
    folderPicks = false
  }: {
    selected: SvelteSet<string>;
    /** the folders are picked like items, by `folder:` and their key */
    folderPicks?: boolean;
  } = $props();

  const item = (id: string): Item => ({ id });
  const at = (key: string, label: string) => ({ key, path: key, label });
  const b = folderNode(at("A/B", "B"), [item("b1")], [], () => 1);
  const a = folderNode(at("A", "A"), [item("a1"), item("a2")], [b], () => 1);
  const c = folderNode(at("C", "C"), [item("c1")], [], () => 1);
  const expansion = new Expansion(false);
  const lines = $derived(treeLines([a, c], expansion.isOpen, (i) => i.id));
</script>

<FileTree
  {lines}
  {expansion}
  label="Files"
  columns="auto 1fr"
  {selected}
  idOf={(i) => i.id}
  folderId={folderPicks ? (node) => `folder:${node.key}` : undefined}>
  {#snippet header(all)}
    <span role="columnheader"
      >{#if all}<SelectAll {all} />{/if}</span>
    <span role="columnheader">Name</span>
  {/snippet}
  {#snippet folder(node)}<span>{node.label}</span>{/snippet}
  {#snippet item(i)}<span role="gridcell">{i.id}</span>{/snippet}
  {#snippet menu(line)}<Menu.Item>Menu of {line.key}</Menu.Item>{/snippet}
</FileTree>
