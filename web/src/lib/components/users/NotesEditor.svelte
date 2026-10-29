<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import { untrack } from "svelte";
  import { fade } from "svelte/transition";

  import { api, byUser } from "#lib/api/client.js";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { motionFast } from "#lib/motion.js";
  import { call } from "#lib/request.js";

  // our private notes about a user, saved as typing pauses, with a tick once saved
  type Props = { username: string; notes: string; disabled: boolean };
  let { username, notes, disabled }: Props = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(notes);
  // svelte-ignore state_referenced_locally
  let saved = $state(notes);
  let status = $state<"" | "saving" | "saved" | string>("");

  /** how long typing pauses before the notes are saved */
  const SAVE_AFTER_MS = 600;
  /** how long the tick shows after a save */
  const TICK_MS = 2_000;

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
    if (value === saved) return;
    const timer = setTimeout(async () => {
      status = "saving";
      const outcome = await call(() =>
        api.PUT("/api/v0/users/{username}/notes", {
          params: byUser(username),
          body: { notes: value }
        })
      );
      if (outcome.ok) {
        saved = value;
        status = "saved";
        setTimeout(() => {
          if (status === "saved") status = "";
        }, TICK_MS);
      } else status = outcome.error;
    }, SAVE_AFTER_MS);
    return () => clearTimeout(timer);
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
    {#if status === "saving"}
      <Spinner class="size-3" />Saving
    {:else if status === "saved"}
      <span
        class="flex items-center gap-1 text-success"
        transition:fade={{ duration: motionFast() }}><CheckIcon class="size-3.5" />Saved</span>
    {:else if status}
      <span class="text-danger">{status}</span>
    {/if}
  </p>
</div>
