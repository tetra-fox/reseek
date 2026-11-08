<script lang="ts">
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { enqueueJob } from "#lib/files/downloads.js";
  import { formatCount, plural } from "#lib/format.js";
  import { parsePaste } from "#lib/transfers/add.js";

  // a download by hand: a user and a path, or links and search results pasted in
  let { open = $bindable() }: { open: boolean } = $props();

  let username = $state("");
  let path = $state("");
  let paste = $state("");
  let busy = $state(false);

  const pasted = $derived(parsePaste(paste));
  const wanted = $derived.by(() => {
    // a copy, so the typed file is not added to what the paste was read as
    const found = new Map(pasted.wanted);
    const user = username.trim();
    const file = path.trim();
    if (user && file) found.set(user, [...(found.get(user) ?? []), { filename: file, size: 0 }]);
    return found;
  });
  const count = $derived([...wanted.values()].reduce((n, files) => n + files.length, 0));

  const submit = async () => {
    busy = true;
    const result = await enqueueJob(
      [...wanted].map(([user, files]) => ({ username: user, files }))
    );
    busy = false;
    const failed = result.kind !== "done";
    toast(result.text, failed ? "error" : "info");
    if (!failed) {
      username = "";
      path = "";
      paste = "";
      open = false;
    }
  };
</script>

<Dialog
  bind:open
  {busy}
  title="Add downloads"
  description="Enter a username and file path, or paste slsk:// links or search results as JSON">
  <form
    id="add-download"
    class="flex flex-col gap-group"
    onsubmit={(e) => {
      e.preventDefault();
      void submit();
    }}>
    <label class="flex flex-col gap-1 text-sm">
      Username
      <Input bind:value={username} autocomplete="off" data-lpignore="true" />
    </label>
    <label class="flex flex-col gap-1 text-sm">
      Remote path
      <Input bind:value={path} placeholder="@@share\Music\Artist\Album\01 Track.flac" />
    </label>
    <label class="flex flex-col gap-1 text-sm">
      Or paste
      <Textarea
        bind:value={paste}
        rows={4}
        class="font-mono text-xs"
        aria-describedby={pasted.unread ? "add-unread" : undefined} />
      {#if pasted.unread}
        <span id="add-unread" class="text-xs text-warning">
          {plural(pasted.unread, "line", "lines")} could not be read
        </span>
      {/if}
    </label>
  </form>
  {#snippet footer()}
    <Button variant="ghost" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
    <Button type="submit" form="add-download" disabled={busy || !count}>
      Download {count ? formatCount(count) : ""}
    </Button>
  {/snippet}
</Dialog>
