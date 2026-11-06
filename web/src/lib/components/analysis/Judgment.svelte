<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";

  import {
    judge,
    type Judgment,
    JUDGMENTS,
    type Report,
    type Target,
    targetKey
  } from "#lib/analysis/analysis.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { formatDate } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";

  // the user's own judgment of the file, which lists show in place of the analyzer's reading:
  // genuine, fake or not decided, with a note, saved with a tick when it is kept
  type Props = { target: Target; report: Report; onsaved: () => void };

  let { target, report, onsaved }: Props = $props();

  /** the choices, `undecided` standing for no judgment */
  const CHOICES = [
    { id: "legit", label: JUDGMENTS.legit },
    { id: "fake", label: JUDGMENTS.fake },
    { id: "undecided", label: "Not decided" }
  ] as const;
  let choice = $state<Judgment | null>(null);
  let note = $state("");
  let saving = $state(false);
  let saved = $state(false);
  // the form follows the judgment kept; the tick goes when another file is shown
  $effect(() => {
    choice = report.override?.verdict ?? null;
    note = report.override?.note ?? "";
  });
  $effect(() => {
    void targetKey(target);
    saved = false;
  });
  const changed = $derived(
    choice !== (report.override?.verdict ?? null) || note.trim() !== (report.override?.note ?? "")
  );
  const write = $derived(session.can("ReadWrite"));

  const save = async () => {
    saving = true;
    const outcome = await judge(target, choice, note);
    saving = false;
    if (!outcome.ok) return void toast(`Could not save the judgment: ${outcome.error}`, "error");
    saved = true;
    onsaved();
  };
</script>

<fieldset class="flex flex-col gap-2" disabled={!write || saving}>
  <legend class="mb-1 text-sm font-medium">Your judgment</legend>
  <Segmented
    size="sm"
    label="Your judgment"
    options={CHOICES}
    bind:value={
      () => choice ?? "undecided",
      (id) => {
        choice = id === "undecided" ? null : id;
        saved = false;
      }
    } />
  <label class="flex flex-col gap-1 text-sm">
    <span class="text-fg-muted">Note</span>
    <Textarea
      bind:value={note}
      rows={2}
      placeholder="What you heard or saw"
      disabled={choice === null}
      oninput={() => (saved = false)} />
  </label>
  <div class="flex items-center gap-2">
    <Button size="sm" disabled={!changed} onclick={() => void save()}>Save judgment</Button>
    {#if saved && !changed}
      <span class="flex items-center gap-1 text-sm text-success" role="status"
        ><CheckIcon class="size-4" />Saved</span>
    {:else if report.override}
      <span class="text-xs text-fg-muted">Saved {formatDate(report.override.at)}</span>
    {/if}
  </div>
</fieldset>
