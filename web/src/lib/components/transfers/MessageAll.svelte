<script lang="ts">
  import { api, byUser } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { plural } from "#lib/format.js";
  import { call } from "#lib/request.js";
  import { toast } from "#lib/toast.svelte.js";

  type Props = {
    open: boolean;
    /** the users with an upload in the list as it is filtered */
    usernames: string[];
  };

  let { open = $bindable(), usernames }: Props = $props();

  let text = $state("");
  let sent = $state(0);
  let sending = $state(false);

  /** One private message to each user, one after another, then what came of it */
  const send = async () => {
    const message = text.trim();
    if (!message) return;
    sending = true;
    sent = 0;
    const failed: string[] = [];
    for (const username of usernames) {
      const outcome = await call(() =>
        api.POST("/api/v0/conversations/{username}", { params: byUser(username), body: message })
      );
      if (outcome.ok) sent += 1;
      else failed.push(`${username}: ${outcome.error}`);
    }
    sending = false;
    if (failed.length) {
      toast(
        `Sent to ${sent} of ${plural(usernames.length, "user", "users")}. Could not send to ${failed.join("; ")}`,
        "error"
      );
      return;
    }
    toast(`Sent to ${plural(sent, "user", "users")}`);
    text = "";
    open = false;
  };
</script>

<Dialog
  bind:open
  title="Message everyone downloading from you"
  description={`A private message to each of the ${plural(usernames.length, "user", "users")} in the filtered list.`}>
  <form
    id="message-all"
    onsubmit={(e) => {
      e.preventDefault();
      void send();
    }}>
    <Textarea bind:value={text} rows={4} aria-label="Message" required />
  </form>
  {#if sending}
    <p class="text-sm text-fg-muted" role="status">Sent {sent} of {usernames.length}...</p>
  {/if}
  {#snippet footer()}
    <Button variant="outline" onclick={() => (open = false)}>Cancel</Button>
    <Button
      type="submit"
      form="message-all"
      disabled={sending || !text.trim() || !usernames.length}>
      Send to {plural(usernames.length, "user", "users")}
    </Button>
  {/snippet}
</Dialog>
