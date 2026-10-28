<script lang="ts">
  import PlusIcon from "@lucide/svelte/icons/plus";
  import XIcon from "@lucide/svelte/icons/x";
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import {
    type Preset,
    presetsOf,
    savePresets,
    whyNotSaved,
    withPreset
  } from "#lib/search/presets.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { undoable } from "#lib/undo.js";

  // the named filters kept by the daemon, one click puts one in the filter box
  let { filter = $bindable() }: { filter: string } = $props();

  const presets = $derived(presetsOf(app.options));
  const why = $derived(whyNotSaved(app.options, session.can("Administrator")));
  /** removed presets whose undo still waits, off the row meanwhile */
  const waiting = new SvelteSet<string>();
  /** removed presets saved without them, off the row until the options come back without them */
  const removed = new SvelteSet<string>();
  let saving = $state(false);
  let name = $state("");
  let text = $state("");
  let busy = $state(false);

  // a removal the options no longer list is done, so a preset of that name added again elsewhere
  // shows
  $effect(() => {
    const listed = new Set(presets.map((p) => p.name));
    for (const name of removed) if (!listed.has(name)) removed.delete(name);
  });

  const shown = $derived(presets.filter((p) => !waiting.has(p.name) && !removed.has(p.name)));
  const replaces = $derived(presets.some((p) => p.name === name.trim()));

  const ask = () => {
    const current = presets.find((p) => p.filter === filter.trim());
    name = current?.name ?? "";
    text = filter.trim();
    saving = true;
  };

  const save = async () => {
    busy = true;
    const kept = presets.filter((p) => !removed.has(p.name));
    const outcome = await savePresets(withPreset(kept, { name: name.trim(), filter: text.trim() }));
    if (outcome.ok) removed.delete(name.trim());
    busy = false;
    if (!outcome.ok) return void toast(outcome.error, "error");
    saving = false;
    toast(`Saved preset ${name.trim()}`);
  };

  /**
   * Takes a preset off the row at once and out of the daemon once its undo has passed. Each
   * removal saves the presets as they are then without its own name and those already removed,
   * so removing several in a row keeps every one off, and each undo puts back its own
   */
  const remove = (preset: Preset) => {
    waiting.add(preset.name);
    undoable(
      `Removed preset ${preset.name}`,
      async () => {
        const outcome = await savePresets(
          presets.filter((p) => p.name !== preset.name && !removed.has(p.name))
        );
        waiting.delete(preset.name);
        if (outcome.ok) removed.add(preset.name);
        else toast(outcome.error, "error");
      },
      () => waiting.delete(preset.name)
    );
  };
</script>

<div class="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter presets">
  <span class="text-xs text-fg-muted">Presets</span>
  {#each shown as preset (preset.name)}
    {@const on = filter.trim() === preset.filter}
    <span
      class={[
        "group flex items-center rounded-full border text-xs",
        on ? "border-accent bg-accent-soft text-accent-soft-fg" : "text-fg-muted"
      ]}>
      <button
        type="button"
        class="py-0.5 pl-2.5 hover:text-fg"
        aria-pressed={on}
        title={preset.filter}
        onclick={() => (filter = on ? "" : preset.filter)}>{preset.name}</button>
      {#if !why}
        <button
          type="button"
          class="rounded-full p-1 pr-1.5 row-action"
          aria-label={`Remove preset ${preset.name}`}
          onclick={() => remove(preset)}>
          <XIcon class="size-3" />
        </button>
      {:else}
        <span class="pr-2.5"></span>
      {/if}
    </span>
  {/each}
  <Button
    variant="ghost"
    size="sm"
    class="h-6 text-xs"
    disabled={!!why || !filter.trim()}
    title={why ?? (filter.trim() ? "Save the current filter as a preset" : "Type a filter first")}
    onclick={ask}>
    <PlusIcon />Save as preset
  </Button>
</div>

<Dialog bind:open={saving} title="Save a filter preset">
  <form
    id="save-preset"
    class="flex flex-col gap-3"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}>
    <label class="flex flex-col gap-1 text-sm">
      Name
      <Input bind:value={name} required maxlength={40} autocomplete="off" />
    </label>
    <label class="flex flex-col gap-1 text-sm">
      Filter
      <Input bind:value={text} required class="font-mono" />
    </label>
    {#if replaces}
      <Notice kind="info">A preset named {name.trim()} already exists and will be replaced.</Notice>
    {/if}
  </form>
  {#snippet footer()}
    <Button variant="outline" onclick={() => (saving = false)}>Cancel</Button>
    <Button type="submit" form="save-preset" disabled={busy || !name.trim() || !text.trim()}>
      {replaces ? "Replace" : "Save"}
    </Button>
  {/snippet}
</Dialog>
