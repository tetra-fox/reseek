<script lang="ts">
  import { api, byUser } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { formatCount, plural } from "#lib/format.js";
  import { call } from "#lib/request.js";

  type Props = {
    open: boolean;
    /** who the message goes to */
    usernames: string[];
    title: string;
    /** who `usernames` are, in a few words */
    description: string;
  };

  let { open = $bindable(), usernames, title, description }: Props = $props();

  let text = $state("");
  let sent = $state(0);
  /** how many users the message is being sent to */
  let total = $state(0);
  let sending = $state(false);
  /** Cancel was pressed while sending: nobody after the one being sent to is sent the message */
  let canceled = false;

  /** One private message to each user, one after another, then what came of it */
  const send = async () => {
    const message = text.trim();
    if (!message) return;
    // the users as they were when Send was pressed, though the list can change while sending
    const to = usernames;
    sending = true;
    canceled = false;
    sent = 0;
    total = to.length;
    const failed: string[] = [];
    for (const username of to) {
      if (canceled) break;
      const outcome = await call(() =>
        api.POST("/api/v0/conversations/{username}", { params: byUser(username), body: message })
      );
      if (outcome.ok) sent += 1;
      else failed.push(`${username}: ${outcome.error}`);
    }
    sending = false;
    const of = `Sent to ${formatCount(sent)} of ${plural(to.length, "user", "users")}`;
    // a send that failed or was canceled part way keeps the text and the dialog, to send again
    if (failed.length) {
      toast(`${of}. Could not send to ${failed.join("; ")}`, "error");
      return;
    }
    if (canceled) {
      toast(`${of} before canceling`);
      return;
    }
    toast(`Sent to ${plural(sent, "user", "users")}`);
    text = "";
    open = false;
  };

  const cancel = () => {
    if (sending) canceled = true;
    else open = false;
  };
</script>

<Dialog bind:open busy={sending} {title} {description}>
  <form
    id="message-all"
    onsubmit={(e) => {
      e.preventDefault();
      void send();
    }}>
    <Textarea bind:value={text} rows={4} aria-label="Message" required />
  </form>
  {#if sending}
    <p class="text-sm text-fg-muted" role="status">
      Sent {formatCount(sent)} of {formatCount(total)}...
    </p>
  {/if}
  {#snippet footer()}
    <Button variant="outline" onclick={cancel}>Cancel</Button>
    <Button
      type="submit"
      form="message-all"
      disabled={sending || !text.trim() || !usernames.length}>
      Send to {plural(usernames.length, "user", "users")}
    </Button>
  {/snippet}
</Dialog>
