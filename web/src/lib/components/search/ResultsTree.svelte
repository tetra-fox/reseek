<script lang="ts">
  import { goto } from "$app/navigation";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import EyeOffIcon from "@lucide/svelte/icons/eye-off";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import LinkIcon from "@lucide/svelte/icons/link";
  import ListIcon from "@lucide/svelte/icons/list";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import FileCells from "#lib/components/files/FileCells.svelte";
  import FileHeader from "#lib/components/files/FileHeader.svelte";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import FolderFiles from "#lib/components/files/FolderFiles.svelte";
  import FolderLine from "#lib/components/files/FolderLine.svelte";
  import SelectionBar from "#lib/components/files/SelectionBar.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Flag from "#lib/components/ui/Flag.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import GroupBadge from "#lib/components/users/GroupBadge.svelte";
  import Presence from "#lib/components/users/Presence.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { fileColumns, fileTemplate, fileWidth } from "#lib/files/columns.js";
  import { asItem, type Enqueued, enqueue, followDownloads } from "#lib/files/downloads.js";
  import { folderLinks } from "#lib/files/links.js";
  import { fetchFolder } from "#lib/files/remote.js";
  import { slskLink } from "#lib/files/slsk.js";
  import { formatAttributes, getFileName, plural } from "#lib/format.js";
  import { inspector } from "#lib/inspector/inspector.svelte.js";
  import { href } from "#lib/nav.js";
  import { offerFields, type Picked, userFacts } from "#lib/search/facts.js";
  import { type Result, type ResultFile, resultFiles } from "#lib/search/results.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import {
    type FolderNode,
    folderNode,
    folderTree,
    itemsUnder,
    tickedCounts,
    treeLines
  } from "#lib/tree/folders.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

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
  };

  let { view, searchId, expansion, connected, canWrite, ondismiss, inspect }: Props = $props();

  const write = $derived(canWrite && connected);
  const selected = new SvelteSet<string>();
  let asking = $state<string | null>(null);
  let flat = $state<{ username: string; folder: string; files: Schemas["File"][] } | null>(null);
  let flatOpen = $state(false);

  followDownloads();

  const idOf = (f: Item) => `${f.result}\u0000${f.filename}`;
  /** each response as a line for its user with its files' folders under it */
  const roots = $derived(
    view.map((r) => {
      const files: Item[] = resultFiles(r.response).map((f) => ({
        ...f,
        owner: r.response.username,
        result: r.key
      }));
      const size = (f: Item) => f.size;
      const locked = (f: Item) => f.locked;
      const folders = folderTree(files, (f) => f.filename, size, locked, r.key);
      const at = { key: `user:${r.key}`, path: r.response.username, label: r.response.username };
      return folderNode(at, [], folders, size, locked);
    })
  );
  const results = $derived(new Map(view.map((r) => [`user:${r.key}`, r])));
  const lines = $derived(treeLines(roots, expansion.isOpen, idOf));
  const all = $derived(roots.flatMap((root) => itemsUnder(root)));
  const byId = $derived(new Map(all.map((f) => [idOf(f), f])));
  const chosen = $derived([...selected].flatMap((id) => byId.get(id) ?? []));
  /** ticked files under each folder, by its key, and under each user, by their result */
  const ticked = $derived(tickedCounts(selected, (id) => id));
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

  /** Downloads files, one batch per user, and says what came of it */
  const download = async (files: Item[]): Promise<Enqueued> => {
    const byUser = new Map<string, Item[]>();
    for (const f of files) {
      if (f.locked) continue;
      const listed = byUser.get(f.owner);
      if (listed) listed.push(f);
      else byUser.set(f.owner, [f]);
    }
    let last: Enqueued = { kind: "done", text: "" };
    for (const [username, mine] of byUser) {
      last = await enqueue({ username, files: mine.map((f) => asItem(f)), searchId });
      if (last.kind !== "done") break;
    }
    return last;
  };
  const downloadNow = async (files: Item[]) => {
    const outcome = await download(files);
    toast(outcome.text, outcome.kind === "done" ? "info" : "error");
  };

  /** Lists every file under a folder, asked of the user so files the search missed are in */
  const showFolder = async (username: string, folder: string) => {
    asking = `${username}\u0000${folder}`;
    const outcome = await fetchFolder(username, folder);
    asking = null;
    if (!outcome.ok) {
      toast(`Could not load the folder: ${outcome.error}`, "error");
      return;
    }
    flat = { username, folder, files: outcome.data.flatMap((d) => d.files) };
    flatOpen = true;
  };
</script>

{#snippet userLine(node: FolderNode<Item>, result: Result)}
  {@const response = result.response}
  {@const name = response.username}
  {@const user = users.get(name)}
  {@const on = tickedOf.get(result.key) ?? 0}
  {#if node.unlocked}
    {@const state = tickState(node, on)}
    <Checkbox
      aria-label={`Select every file from ${name}`}
      checked={state.checked}
      indeterminate={state.indeterminate}
      onCheckedChange={state.ontick} />
  {/if}
  <span
    class={[
      "size-2.5 shrink-0 rounded-full",
      response.hasFreeUploadSlot ? "bg-success" : "bg-warning"
    ]}
    title={response.hasFreeUploadSlot ? "Has a free upload slot" : "No free upload slot"}></span>
  <span class="flex min-w-0 flex-1 items-center gap-2">
    <Username {name} />
    {#if user}<Presence presence={user.presence} />{/if}
    {#if user?.group.color}<GroupBadge group={user.group} />{/if}
    <Flag code={response.country} />
    {#if user?.block}<Badge tone="danger">blocked</Badge>{/if}
    <span class="truncate text-xs text-fg-muted max-md:hidden">
      {userFacts(response)} · {plural(node.count, "file", "files")}
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
          <Menu.Item onSelect={() => goto(href(`/browse?user=${encodeURIComponent(name)}`))}
            ><FolderOpenIcon />Browse</Menu.Item>
          {#if canWrite}
            <Menu.Item onSelect={() => goto(href(`/searches?user=${encodeURIComponent(name)}`))}
              ><SearchIcon />More from this user</Menu.Item>
          {/if}
          {#if write}
            <Menu.Item onSelect={() => void downloadNow(itemsUnder(node))}
              ><DownloadIcon />Download all</Menu.Item>
          {/if}
          <Menu.Item onSelect={() => ondismiss(name)}><EyeOffIcon />Hide this result</Menu.Item>
        </Menu.Content>
      </Menu.LazyRoot>
    {:else}
      <Button variant="ghost" size="sm" href={href(`/browse?user=${encodeURIComponent(name)}`)}>
        <FolderOpenIcon />Browse
      </Button>
      {#if canWrite}
        <Button variant="ghost" size="sm" href={href(`/searches?user=${encodeURIComponent(name)}`)}>
          <SearchIcon />More from this user
        </Button>
      {/if}
      {#if write}
        <Button variant="ghost" size="sm" onclick={() => void downloadNow(itemsUnder(node))}>
          <DownloadIcon />Download all
        </Button>
      {/if}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Hide this result"
        title="Hide this result"
        onclick={() => ondismiss(name)}>
        <EyeOffIcon />
      </Button>
    {/if}
  </span>
{/snippet}

<FileTree
  {lines}
  {expansion}
  label="Search results"
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
  {#snippet header()}
    <FileHeader {columns} />
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
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="View all files in this folder"
            title="View all files in this folder"
            disabled={asking !== null}
            onclick={() => void showFolder(owner, node.path)}>
            {#if asking === `${owner}\u0000${node.path}`}<Spinner
                class="size-4" />{:else}<ListIcon />{/if}
          </Button>
        {/snippet}
        {#snippet menu()}
          <Menu.Item onSelect={() => goto(folderLinks(owner, node.path).browse)}
            ><FolderOpenIcon />Browse this folder</Menu.Item>
          <Menu.Item onSelect={() => void copyText(node.path)}><CopyIcon />Copy path</Menu.Item>
          <Menu.Item onSelect={() => void copyText(slskLink(owner, node.path, true))}
            ><LinkIcon />Copy slsk:// link</Menu.Item>
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
      ondetails={() => {
        inspect(pick(file));
        inspector.show();
      }} />
  {/snippet}
</FileTree>
{#if canWrite && chosen.length}
  <SelectionBar
    files={chosen}
    ondownload={connected ? () => download(chosen) : undefined}
    onclear={() => selected.clear()} />
{/if}

{#if flat}
  <FolderFiles
    bind:open={flatOpen}
    username={flat.username}
    folder={flat.folder}
    files={flat.files}
    {connected} />
{/if}
