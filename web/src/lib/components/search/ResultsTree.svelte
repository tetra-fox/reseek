<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import EyeOffIcon from "@lucide/svelte/icons/eye-off";
  import type { SvelteSet } from "svelte/reactivity";

  import FileCells from "#lib/components/files/FileCells.svelte";
  import FileHeader from "#lib/components/files/FileHeader.svelte";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import FolderLine from "#lib/components/files/FolderLine.svelte";
  import RemoteFileItems from "#lib/components/files/RemoteFileItems.svelte";
  import RemoteFolderItems from "#lib/components/files/RemoteFolderItems.svelte";
  import SelectionBar from "#lib/components/files/SelectionBar.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Flag from "#lib/components/ui/Flag.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import GroupBadge from "#lib/components/users/GroupBadge.svelte";
  import Presence from "#lib/components/users/Presence.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import type { AllFiles } from "#lib/files/allfiles.svelte.js";
  import { fileColumns, fileTemplate, fileWidth } from "#lib/files/columns.js";
  import { asItem, type Enqueued, enqueueJob, followDownloads } from "#lib/files/downloads.js";
  import { formatAttributes, getFileName, plural } from "#lib/format.js";
  import { inspector } from "#lib/inspector/inspector.svelte.js";
  import { offerFields, type Picked, userFacts } from "#lib/search/facts.js";
  import { type Result, type ResultFile, resultFiles } from "#lib/search/results.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import {
    type FolderNode,
    itemsUnder,
    scopedKey,
    tickedCounts,
    treeAt,
    treeLines
  } from "#lib/tree/folders.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { reach } from "#lib/users/actions.js";
  import { queueOf } from "#lib/users/groups.js";
  import { users } from "#lib/users/users.svelte.js";

  import ResultUserItems from "./ResultUserItems.svelte";

  /** A file of a response, with whose it is and the response it came in */
  type Item = ResultFile & { owner: string; result: string };

  type Props = {
    /** the responses shown, in their order */
    view: Result[];
    searchId: string;
    /** which users and folders are open; a user's line is keyed `user:` and its result's key */
    expansion: Expansion;
    /** downloads need the Soulseek server */
    connected: boolean;
    canWrite: boolean;
    ondismiss: (username: string) => void;
    /** focus came to a file, which the inspector shows */
    inspect: (file: Picked) => void;
    /** the dialog that lists every file under a folder */
    allFiles: AllFiles;
    /** the ticked files, by their result's key and path, which the list layout shares */
    selected: SvelteSet<string>;
  };

  let {
    view,
    searchId,
    expansion,
    connected,
    canWrite,
    ondismiss,
    inspect,
    allFiles,
    selected
  }: Props = $props();

  const write = $derived(canWrite && connected);
  let tree = $state<ReturnType<typeof FileTree<Item>> | null>(null);

  followDownloads();

  const idOf = (f: Item) => scopedKey(f.result, f.filename);
  /**
   * each response as a line for its user with its files' folders under it, and in it the files a
   * peer sent with no folder
   */
  const roots = $derived(
    view.map((r) => {
      const files: Item[] = resultFiles(r.response).map((f) => ({
        ...f,
        owner: r.response.username,
        result: r.key
      }));
      const at = { key: `user:${r.key}`, path: r.response.username, label: r.response.username };
      return treeAt(
        at,
        files,
        (f) => f.filename,
        (f) => f.size,
        (f) => f.locked,
        r.key
      );
    })
  );
  const results = $derived(new Map(view.map((r) => [`user:${r.key}`, r])));
  const lines = $derived(treeLines(roots, expansion.isOpen, idOf));
  const all = $derived(roots.flatMap((root) => itemsUnder(root)));
  const byId = $derived(new Map(all.map((f) => [idOf(f), f])));
  const chosen = $derived([...selected].flatMap((id) => byId.get(id) ?? []));
  /** ticked files under each folder, by its key, and under each user, by their result */
  const ticked = $derived(tickedCounts(selected));
  const tickedOf = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const id of selected) {
      const result = id.slice(0, id.indexOf("\u0000"));
      counts.set(result, (counts.get(result) ?? 0) + 1);
    }
    return counts;
  });
  const attributed = $derived(all.some((f) => f.suspect || formatAttributes(f)));
  const columns = $derived(fileColumns({ quality: attributed, phone: viewport.phone }));

  /** A file for the inspector, with what its response says of its user */
  const pick = (f: Item): Picked => {
    const response = results.get(`user:${f.result}`)?.response;
    return {
      file: f,
      username: f.owner,
      facts: response
        ? offerFields(response.uploadSpeed, response.hasFreeUploadSlot, response.queueLength)
        : []
    };
  };

  /** Whose share a folder is in, from its key, which starts with its response's */
  const ownerOf = (node: FolderNode<Item>) =>
    results.get(`user:${node.key.slice(0, node.key.indexOf("\u0000"))}`)?.response.username ?? "";

  /** Ticks or unticks every file under a line that can be ticked */
  const tick = (node: FolderNode<Item>, on: boolean) => {
    for (const f of itemsUnder(node)) {
      if (f.locked) continue;
      if (on) selected.add(idOf(f));
      else selected.delete(idOf(f));
    }
  };
  const tickState = (node: FolderNode<Item>, on: number) => ({
    checked: on > 0 && on === node.unlocked,
    indeterminate: on > 0 && on < node.unlocked,
    ontick: (yes: boolean) => tick(node, yes)
  });

  /** Downloads files as one job, one group per user, and says what came of it */
  const download = (files: Item[]): Promise<Enqueued> => {
    const byUser = new Map<string, Item[]>();
    for (const f of files) {
      if (f.locked) continue;
      const listed = byUser.get(f.owner);
      if (listed) listed.push(f);
      else byUser.set(f.owner, [f]);
    }
    return enqueueJob(
      [...byUser].map(([username, mine]) => ({
        username,
        files: mine.map((f) => asItem(f)),
        searchId
      }))
    );
  };
  const downloadNow = async (files: Item[]) => {
    const outcome = await download(files);
    toast(outcome.text, outcome.kind === "done" ? "info" : "error");
  };

  const details = (file: Item) => {
    inspect(pick(file));
    inspector.show();
  };

  /** Gives the tree DOM focus, without scrolling */
  export const focus = () => tree?.focus();
</script>

{#snippet userItems(node: FolderNode<Item>, name: string)}
  <ResultUserItems
    {name}
    ondownload={write ? () => void downloadNow(itemsUnder(node)) : undefined}
    ondismiss={() => ondismiss(name)} />
{/snippet}

{#snippet folderItems(node: FolderNode<Item>)}
  {@const owner = ownerOf(node)}
  <RemoteFolderItems
    folder={node.path}
    username={owner}
    ondownload={write && node.unlocked ? () => void downloadNow(itemsUnder(node)) : undefined}
    onallfiles={() => void allFiles.show(owner, node.path)} />
{/snippet}

{#snippet userLine(node: FolderNode<Item>, result: Result)}
  {@const response = result.response}
  {@const name = response.username}
  {@const user = users.get(name)}
  {@const queue = user ? queueOf(user) : null}
  {@const on = tickedOf.get(result.key) ?? 0}
  {#if node.unlocked}
    {@const state = tickState(node, on)}
    <Checkbox
      aria-label={`Select every file from ${name}`}
      checked={state.checked}
      indeterminate={state.indeterminate}
      onCheckedChange={state.ontick} />
  {/if}
  <Tooltip
    text={response.hasFreeUploadSlot ? "Free upload slot" : "No free upload slot"}
    dense
    spoken>
    {#snippet child({ props })}
      <span
        {...props}
        class={[
          "size-2.5 shrink-0 rounded-full",
          response.hasFreeUploadSlot ? "bg-success" : "bg-warning"
        ]}></span>
    {/snippet}
  </Tooltip>
  <!-- the facts give way before the name does -->
  <span class="name-count flex-1 gap-2">
    <span class="flex min-w-0 items-center gap-2">
      <Username {name} />
      {#if user}<Presence presence={user.presence} />{/if}
      {#if queue?.color}<GroupBadge group={queue} queue />{/if}
      <Flag code={response.country} />
      {#if user?.block}<Badge tone="danger">blocked</Badge>{/if}
    </span>
    <span class="text-xs text-fg-muted max-md:hidden">
      <span>{plural(node.count, "file", "files")} · {userFacts(response)}</span>
    </span>
  </span>
  <span class="flex shrink-0 items-center gap-0.5 row-action">
    {#if viewport.phone}
      <Menu.LazyRoot>
        {#snippet trigger({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label={`More for ${name}`}>
            <EllipsisIcon />
          </Button>
        {/snippet}
        <Menu.Content align="end" class="min-w-56">
          {@render userItems(node, name)}
        </Menu.Content>
      </Menu.LazyRoot>
    {:else}
      {#each reach(name, { only: ["shares", "search"] }) as action (action.id)}
        <Button variant="ghost" size="sm" href={action.href}><action.icon />{action.label}</Button>
      {/each}
      {#if write}
        <Button variant="ghost" size="sm" onclick={() => void downloadNow(itemsUnder(node))}>
          <DownloadIcon />Download all
        </Button>
      {/if}
      <Tooltip text="Hide result">
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon-sm"
            aria-label="Hide result"
            onclick={() => ondismiss(name)}>
            <EyeOffIcon />
          </Button>
        {/snippet}
      </Tooltip>
    {/if}
  </span>
{/snippet}

<FileTree
  bind:this={tree}
  {lines}
  {expansion}
  label="Search results"
  place={`search.${searchId}.tree`}
  columns={fileTemplate(columns)}
  width={fileWidth(columns)}
  {selected}
  {idOf}
  tickable={(f) => !f.locked}
  nameOf={(line) => (line.kind === "folder" ? line.node.label : getFileName(line.item.filename))}
  slot={(depth) => (viewport.phone ? Math.min(depth, 1) : depth)}
  onopen={write ? (f) => void downloadNow([f]) : undefined}
  onmiddleclick={write ? (f) => void downloadNow([f]) : undefined}
  inspect={(f) => inspect(pick(f))}>
  {#snippet header(all)}
    <FileHeader {columns} {all} />
  {/snippet}
  {#snippet menu(line)}
    {#if line.kind === "item"}
      <RemoteFileItems
        path={line.item.filename}
        username={line.item.owner}
        ondownload={write && !line.item.locked ? () => void downloadNow([line.item]) : undefined}
        ondetails={() => details(line.item)} />
    {:else if line.depth === 0}
      {@render userItems(line.node, results.get(line.node.key)?.response.username ?? "")}
    {:else}
      {@render folderItems(line.node)}
    {/if}
  {/snippet}
  {#snippet folder(node, depth)}
    {@const result = depth === 0 ? results.get(node.key) : undefined}
    {#if result}
      {@render userLine(node, result)}
    {:else}
      {@const owner = ownerOf(node)}
      <FolderLine
        {node}
        open={expansion.isOpen(node.key)}
        depth={depth - 1}
        tick={tickState(node, ticked.get(node.key) ?? 0)}
        ondownload={write && node.unlocked ? () => void downloadNow(itemsUnder(node)) : undefined}>
        {#snippet actions()}
          <Tooltip text={A.allFiles.label}>
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label={A.allFiles.label}
                disabled={allFiles.asking !== null}
                onclick={() => void allFiles.show(owner, node.path)}>
                {#if allFiles.isAsking(owner, node.path)}<Spinner
                    class="size-4" />{:else}<A.allFiles.icon />{/if}
              </Button>
            {/snippet}
          </Tooltip>
        {/snippet}
        {#snippet menu()}
          {@render folderItems(node)}
        {/snippet}
      </FolderLine>
    {/if}
  {/snippet}
  {#snippet item(file, _depth, tickRow)}
    <FileCells
      {file}
      username={file.owner}
      {columns}
      selected={selected.has(idOf(file))}
      ontick={tickRow}
      ondownload={write && !file.locked ? () => void downloadNow([file]) : undefined}
      ondetails={() => details(file)} />
  {/snippet}
</FileTree>
{#if canWrite && chosen.length}
  <SelectionBar
    files={chosen}
    ondownload={connected ? () => download(chosen) : undefined}
    onclear={() => selected.clear()} />
{/if}
