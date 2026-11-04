<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import { untrack } from "svelte";
  import { fade } from "svelte/transition";

  import { api, byUser } from "#lib/api/client.js";
  import { Autosave } from "#lib/autosave.svelte.js";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { call } from "#lib/request.js";
  import { motionFast } from "#lib/ui/motion.js";

  // our private notes about a user, saved as typing pauses, with a tick once saved
  type Props = { username: string; notes: string; disabled: boolean };
  let { username, notes, disabled }: Props = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(notes);
  /** the notes as the daemon holds them, which the text is compared with */
  // svelte-ignore state_referenced_locally
  let saved = $state(notes);

  /** how long typing pauses before the notes are saved */
  const SAVE_AFTER_MS = 600;
  /** how long the tick shows after a save */
  const TICK_MS = 2_000;

  const saving = new Autosave<string>(async (value) => {
    const outcome = await call(() =>
      api.PUT("/api/v0/users/{username}/notes", {
        params: byUser(username),
        body: { notes: value }
      })
    );
    if (outcome.ok) saved = value;
    return outcome;
  }, SAVE_AFTER_MS);

  // the saved notes changed elsewhere and nothing is being typed: show them
  $effect(() => {
    const from = notes;
    untrack(() => {
      if (text === saved) text = from;
      saved = from;
    });
  });

  $effect(() => {
    const value = text;
    if (value === saved) untrack(() => saving.cancel());
    else untrack(() => saving.change(value));
  });

  let tick = $state(false);
  $effect(() => {
    if (saving.state !== "saved") return;
    tick = true;
    const timer = setTimeout(() => (tick = false), TICK_MS);
    return () => {
      clearTimeout(timer);
      tick = false;
    };
  });
</script>

<div class="flex flex-col gap-1">
  <Textarea
    aria-label="Notes"
    class="min-h-24 text-sm"
    placeholder={disabled ? "" : "Not shown to the user"}
    bind:value={text}
    {disabled} />
  <p class="flex h-4 items-center gap-1 text-xs text-fg-muted" role="status">
    {#if saving.state === "saving"}
      <Spinner class="size-3" />Saving
    {:else if tick}
      <span
        class="flex items-center gap-1 text-success"
        transition:fade={{ duration: motionFast() }}><CheckIcon class="size-3.5" />Saved</span>
    {:else if saving.state === "failed"}
      <span class="text-danger">{saving.error}</span>
    {/if}
  </p>
</div>
