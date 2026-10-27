<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import ListIcon from "@lucide/svelte/icons/list";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import FileList from "#lib/components/files/FileList.svelte";
  import FolderFiles from "#lib/components/files/FolderFiles.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { asItem, type Enqueued, enqueue } from "#lib/downloads.js";
  import { fetchFolder } from "#lib/folders.js";
  import { formatBytes, plural } from "#lib/format.js";
  import { href } from "#lib/nav.js";
  import { folders, type Result, type ResultFile } from "#lib/search/results.js";
  import { toast } from "#lib/toast.svelte.js";
  import { users } from "#lib/users.svelte.js";

  type Props = {
    result: Result;
    searchId: string;
    /** the page's "fold results"; each card can be unfolded on its own after */
    folded: boolean;
    /** downloads need the Soulseek server */
    connected: boolean;
    canWrite: boolean;
    ondismiss: () => void;
  };

  let { result, searchId, folded, connected, canWrite, ondismiss }: Props = $props();

  const response = $derived(result.response);
  const name = $derived(response.username);
  const user = $derived(users.get(name));

  let fold = $derived(folded);
  const selected = new SvelteSet<string>();
  /** folders replaced by what the user sent when asked for the whole folder */
  const fetched = new SvelteMap<string, ResultFile[]>();
  let asking = $state<string | null>(null);
  let downloading = $state(false);
  let status = $state<Enqueued | null>(null);
  let flat = $state<{ folder: string; files: Schemas["File"][] } | null>(null);
  let flatOpen = $state(false);

  $effect(() => void users.load(name));

  const groups = $derived(
    folders(response).map((f) => {
      const replaced = fetched.get(f.name);
      return replaced ? { ...f, files: replaced } : f;
    })
  );
  const byName = $derived(new Map(groups.flatMap((g) => g.files.map((f) => [f.filename, f]))));
  const chosen = $derived([...selected].flatMap((n) => byName.get(n) ?? []));
  const chosenBytes = $derived(chosen.reduce((sum, f) => sum + f.size, 0));

  const select = (filenames: string[], on: boolean) => {
    for (const filename of filenames) {
      if (on) selected.add(filename);
      else selected.delete(filename);
    }
  };

  const download = async (files: Schemas["File"][]) => {
    downloading = true;
    status = null;
    status = await enqueue({ username: name, files: files.map((f) => asItem(f)), searchId });
    downloading = false;
    if (status.kind !== "done") toast(status.text, "error");
  };

  /** Asks the user for the whole folder and lists what they send in place of the results */
  const askFolder = async (folder: string) => {
    asking = folder;
    const outcome = await fetchFolder(name, folder);
    asking = null;
    if (!outcome.ok) {
      toast(`Failed to process directory response: ${outcome.error}`, "error");
      return;
    }
    fetched.set(
      folder,
      outcome.data[0].files.map((f) => ({ ...f, locked: false }))
    );
  };

  /** Lists every file under the folder, asked of the user so files the search missed are in */
  const viewAll = async (folder: string) => {
    asking = folder;
    const outcome = await fetchFolder(name, folder);
    asking = null;
    if (!outcome.ok) {
      toast(outcome.error, "error");
      return;
    }
    flat = { folder, files: outcome.data.flatMap((d) => d.files) };
    flatOpen = true;
  };
</script>

<section class="flex flex-col rounded-card border bg-surface shadow-card" data-key={result.key}>
  <header class="flex flex-wrap items-center gap-2 px-4 pt-3">
    <button
      type="button"
      aria-label={fold ? "Unfold" : "Fold"}
      aria-expanded={!fold}
      onclick={() => (fold = !fold)}>
      {#if fold}<ChevronRightIcon class="size-4" />{:else}<ChevronDownIcon class="size-4" />{/if}
    </button>
    <span
      class={["size-2.5 rounded-full", response.hasFreeUploadSlot ? "bg-success" : "bg-warning"]}
      title={response.hasFreeUploadSlot ? "Has a free upload slot" : "No free upload slot"}></span>
    <Username {name} />
    {#if user}
      <span
        class={[
          "size-2 rounded-full",
          user.presence === "Online"
            ? "bg-success"
            : user.presence === "Away"
              ? "bg-warning"
              : "bg-fg-subtle/50"
        ]}
        title={user.presence ?? "Unknown"}></span>
    {/if}
    {#if user?.group.color}
      <span
        class="size-2 rounded-full"
        style:background-color={user.group.color}
        title={`Group ${user.group.name}`}></span>
    {/if}
    {#if user?.block}<Badge tone="danger">blocked</Badge>{/if}
    <div class="ml-auto flex items-center gap-1">
      <Button variant="ghost" size="sm" href={href(`/browse?user=${encodeURIComponent(name)}`)}>
        <FolderOpenIcon />Browse
      </Button>
      {#if canWrite}
        <Button variant="ghost" size="sm" href={href(`/searches?user=${encodeURIComponent(name)}`)}>
          <SearchIcon />More from this user
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={!connected || downloading}
          onclick={() => void download(groups.flatMap((g) => g.files))}>
          <DownloadIcon />Download all
        </Button>
      {/if}
      <Button variant="ghost" size="icon-sm" aria-label="Hide this result" onclick={ondismiss}>
        <XIcon class="text-danger" />
      </Button>
    </div>
  </header>
  <p class="px-4 pb-2 text-xs text-fg-muted">
    Upload Speed: {formatBytes(response.uploadSpeed)}/s, Free Upload Slot: {response.hasFreeUploadSlot
      ? "YES"
      : "NO"}, Queue Length: {response.queueLength}
  </p>
  {#if !fold}
    <div class="flex flex-col divide-y border-t">
      {#each groups as group (group.name)}
        <FileList
          name={group.name}
          files={group.files}
          username={name}
          locked={group.locked}
          {selected}
          onselect={select}
          ondownload={canWrite && connected ? (file) => void download([file]) : undefined}>
          {#snippet footer()}
            <Button
              variant="outline"
              size="sm"
              disabled={asking !== null}
              onclick={() => void askFolder(group.name)}>
              {#if asking === group.name}<Spinner class="size-4" />{:else}<SearchIcon />{/if}
              Search for Additional Files in This Directory
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={asking !== null}
              onclick={() => void viewAll(group.name)}>
              <ListIcon />View all files
            </Button>
            {#if canWrite}
              <Button
                variant="ghost"
                size="sm"
                disabled={!connected || downloading}
                onclick={() => void download(group.files)}>
                <DownloadIcon />Download folder
              </Button>
            {/if}
          {/snippet}
        </FileList>
      {/each}
    </div>
  {/if}
  {#if canWrite && chosen.length}
    <footer class="flex flex-wrap items-center gap-inline border-t px-4 py-3">
      <Button disabled={!connected || downloading} onclick={() => void download(chosen)}>
        <DownloadIcon />Download
        <span class="font-normal opacity-80">
          {plural(chosen.length, "file", "files")}, {formatBytes(chosenBytes)}
        </span>
      </Button>
      {#if downloading}
        <Spinner class="size-4" label="Queueing" />
      {:else if status?.kind === "done"}
        <CheckIcon class="size-4 text-success" aria-label={status.text} />
      {:else if status}
        <span class="flex items-center gap-1 text-sm text-danger">
          <XIcon class="size-4" />{status.text}
        </span>
      {/if}
    </footer>
  {/if}
</section>

{#if flat}
  <FolderFiles
    bind:open={flatOpen}
    username={name}
    folder={flat.folder}
    files={flat.files}
    {connected} />
{/if}
