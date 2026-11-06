<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderPlusIcon from "@lucide/svelte/icons/folder-plus";
  import { untrack } from "svelte";
  import { SvelteMap } from "svelte/reactivity";

  import { api, type Schemas } from "#lib/api/client.js";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { editability, fileOf, managerOf } from "#lib/config/editability.js";
  import { valueAt } from "#lib/config/sections.js";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { entryPath, shareEntry } from "#lib/system/shares.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { folderNode, type FolderNode, treeLines } from "#lib/tree/folders.js";

  // adds a folder of the daemon's machine to the shares, picked from a tree of its folders, then
  // offers the scan that puts it on the network. The paths are the daemon's own, which in a
  // container means the container's
  type Props = {
    /** says beside the button why it can't be used, where nothing near it says so */
    explain?: boolean;
    onadded?: () => void;
  };

  let { explain = true, onadded }: Props = $props();

  const KEY = "shares.directories";

  const admin = $derived(session.can("Administrator"));
  const view = new Loader(() => call(() => api.GET("/api/v0/options/view")));
  $effect(() => {
    if (admin) untrack(() => void view.load());
  });
  $effect(() =>
    live.on("application.options", () => {
      if (admin) void view.load();
    })
  );
  const data = $derived(view.data);
  const remote = $derived(valueAt(data?.values, "remote_configuration") === true);
  const edit = $derived(data ? editability(data, KEY) : null);
  const manager = $derived(data ? managerOf(data, KEY) : null);
  const addable = $derived(remote && edit?.mode === "save");
  const why = $derived.by(() => {
    if (!data || manager) return null;
    if (!remote) return "Turn on remote_configuration to add shares";
    return edit?.mode === "save" ? null : edit?.notice;
  });
  const entries = $derived((valueAt(data?.values, KEY) ?? []) as string[]);

  let open = $state(false);
  /** what each folder read so far holds, by its path, the top of the machine under "" */
  const folders = new SvelteMap<string, Schemas["ShareFolders"]>();
  const expansion = new Expansion(false);
  let topError = $state("");
  let chosen = $state<string | null>(null);
  let alias = $state("");
  let kind = $state<"share" | "exclude">("share");
  let error = $state("");
  let saving = $state(false);
  /** the folder just added, and whether as an exclusion, which the dialog offers a scan for */
  let added = $state.raw<{ path: string; exclude: boolean } | null>(null);

  const read = async (at: string) => {
    const outcome = await call(() =>
      api.GET("/api/v0/shares/folders", { params: { query: at ? { path: at } : {} } })
    );
    if (outcome.ok) folders.set(at, outcome.data);
    else if (at) {
      toast(outcome.error, "error");
      expansion.set(at, false);
    } else topError = outcome.error;
  };

  const start = () => {
    folders.clear();
    expansion.reset(false);
    topError = "";
    chosen = null;
    alias = "";
    kind = "share";
    error = "";
    added = null;
    open = true;
    void read("");
  };

  const nodeOf = (folder: Schemas["ShareFolder"]): FolderNode<never> => {
    const inside = folders.get(folder.path)?.folders ?? [];
    const at = { key: folder.path, path: folder.path, label: folder.name };
    return folderNode(at, [], inside.map(nodeOf), () => 0);
  };
  const lines = $derived(
    treeLines((folders.get("")?.folders ?? []).map(nodeOf), expansion.isOpen, () => "")
  );
  const cut = $derived([...folders.values()].filter((f) => f.truncated));
  const taken = $derived(!!chosen && entries.some((entry) => entryPath(entry) === chosen));

  const add = async () => {
    if (!data || !chosen || taken) return;
    const file = fileOf(data, KEY);
    const path = chosen;
    const exclude = kind === "exclude";
    const entry = shareEntry(path, alias, exclude);
    saving = true;
    const outcome = await call(() =>
      api.PATCH("/api/v0/options/file", {
        body: {
          file,
          revision: data.files.find((f) => f.name === file)?.revision ?? null,
          set: { [KEY]: [...entries, entry] },
          unset: []
        }
      })
    );
    saving = false;
    if (!outcome.ok) {
      const detail = (outcome.body as { detail?: string | null } | undefined)?.detail;
      error = detail ?? outcome.error;
      // a file changed on disk is read again, so adding once more goes through
      void view.load();
      return;
    }
    added = { path, exclude };
    onadded?.();
  };

  const scanning = new Busy();
  const rescan = async () => {
    const outcome = await scanning.run(() =>
      call(() => api.PUT("/api/v0/shares", { params: { query: {} } }))
    );
    if (outcome?.ok) open = false;
  };
</script>

{#if admin}
  <div class="flex flex-col items-start gap-1">
    <Button variant="outline" disabled={!addable} onclick={start}>
      <FolderPlusIcon />
      {manager ? `Shares are managed by ${manager}` : "Add share"}
    </Button>
    {#if explain && why}<p class="text-xs text-fg-muted">{why}</p>{/if}
  </div>

  <Dialog
    bind:open
    busy={saving || scanning.running}
    title={added ? "Share added" : "Add share"}
    description={added ? undefined : "Folders on the machine reseek runs on"}
    class="w-[min(36rem,calc(100vw-2rem))]">
    {#if added}
      <p class="text-sm">
        <code class="font-mono wrap-anywhere">{added.path}</code>
        {added.exclude
          ? "is excluded. A rescan takes its files off the network."
          : "is shared. A rescan puts its files on the network."}
      </p>
    {:else if topError}
      <Notice kind="error">{topError}</Notice>
    {:else if !folders.has("")}
      <Skeleton class="h-72" />
    {:else}
      <FileTree
        {lines}
        {expansion}
        label="Folders"
        columns="minmax(0,1fr)"
        idOf={() => ""}
        height="18rem"
        onopenfolder={(node) => (chosen = node.path)}
        ontoggle={(opened, keys) => {
          // a folder's key is its path
          if (!opened) return;
          for (const at of keys) if (!folders.has(at)) void read(at);
        }}>
        {#snippet folder(node)}
          <FolderIcon class="size-4 shrink-0 text-fg-muted" />
          <button
            type="button"
            class={[
              "min-w-0 truncate text-left hover:underline",
              node.path === chosen && "font-medium text-accent"
            ]}
            aria-pressed={node.path === chosen}
            onclick={() => (chosen = node.path)}>
            {node.label}
          </button>
          {#if node.path === chosen}
            <CheckIcon class="mr-2 ml-auto size-4 shrink-0 text-accent" aria-hidden="true" />
          {/if}
        {/snippet}
        {#snippet item()}{/snippet}
      </FileTree>
      {#each cut as listed (listed.path)}
        <p class="text-xs text-fg-muted">
          <code class="font-mono wrap-anywhere">{listed.path}</code> has more folders than shown
        </p>
      {/each}
      {#if chosen}
        <div class="flex flex-col gap-1 text-sm">
          <code class="font-mono text-xs wrap-anywhere">{chosen}</code>
          {#if taken}<p class="text-xs text-fg-muted">This folder is already in the shares</p>{/if}
        </div>
      {/if}
      <div class="flex flex-wrap items-end gap-group">
        <Segmented
          size="sm"
          label="What to do with the folder"
          options={[
            { id: "share", label: "Share" },
            { id: "exclude", label: "Exclude" }
          ]}
          bind:value={kind} />
        <label class="flex min-w-48 flex-1 flex-col gap-1 text-xs font-medium text-fg-muted">
          Alias, the name others see
          <Input class="h-control-sm" placeholder="The folder's name" bind:value={alias} />
        </label>
      </div>
      {#if kind === "exclude"}
        <p class="text-xs text-fg-muted">Exclude keeps a folder inside a share off the network</p>
      {/if}
      {#if error}<Notice kind="error">{error}</Notice>{/if}
    {/if}
    {#snippet footer()}
      {#if added}
        <Button variant="ghost" disabled={scanning.running} onclick={() => (open = false)}
          >Not now</Button>
        <Button disabled={scanning.running} onclick={() => void rescan()}>Rescan now</Button>
      {:else}
        <Button variant="ghost" disabled={saving} onclick={() => (open = false)}>Cancel</Button>
        <Button disabled={!chosen || taken || saving} onclick={() => void add()}>
          {kind === "exclude" ? "Exclude folder" : "Add share"}
        </Button>
      {/if}
    {/snippet}
  </Dialog>
{/if}
