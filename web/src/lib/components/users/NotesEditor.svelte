<script lang="ts">
  import { untrack } from "svelte";

  import { api, byUser } from "#lib/api/client.js";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { call } from "#lib/request.js";

  // our private notes about a user, saved a second after typing stops
  type Props = { username: string; notes: string; disabled: boolean };
  let { username, notes, disabled }: Props = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(notes);
  // svelte-ignore state_referenced_locally
  let saved = $state(notes);
  let status = $state<"" | "saving" | "saved" | string>("");

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
      } else status = outcome.error;
    }, 1000);
    return () => clearTimeout(timer);
  });
</script>

<div class="flex flex-col gap-1">
  <Textarea
    aria-label="Notes"
    class="min-h-24 text-sm"
    placeholder={disabled ? "" : "Only you see these"}
    bind:value={text}
    {disabled} />
  <p class="h-4 text-xs text-fg-muted" role="status">
    {status === "saving" ? "Saving…" : status === "saved" ? "Saved" : status}
  </p>
</div>
