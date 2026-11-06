<script lang="ts">
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { getFileName, plural } from "#lib/format.js";
  import { sendFiles, sending, sentText } from "#lib/transfers/send.svelte.js";

  // the page's one dialog for sending our shared files to a user who did not ask for them
  let username = $state("");
  let error = $state("");
  let busy = $state(false);

  $effect(() => {
    if (!sending.open) return;
    username = "";
    error = "";
  });

  const what = $derived(
    sending.files.length === 1
      ? getFileName(sending.files[0])
      : plural(sending.files.length, "file", "files")
  );

  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    const to = username.trim();
    if (!to || busy) return;
    busy = true;
    const outcome = await sendFiles(to, sending.files);
    busy = false;
    if (!outcome.ok) return void (error = outcome.error);
    sending.open = false;
    const { queued, failed } = outcome.data;
    toast(sentText(to, queued, failed), queued.length ? "info" : "error");
  };
</script>

<Dialog
  bind:open={sending.open}
  title={`Send ${what}`}
  description="Their client decides whether to take it. Files go out under the usual upload slots and limits."
  {busy}>
  <Form id="send-form" layout="stacked" class="flex flex-col gap-group" onsubmit={submit}>
    <Field label="Send to" for="send-username">
      <Input
        id="send-username"
        placeholder="Username"
        autocomplete="off"
        required
        bind:value={username} />
    </Field>
    {#if error}<Notice kind="error">{error}</Notice>{/if}
  </Form>
  {#snippet footer()}
    <Button variant="ghost" disabled={busy} onclick={() => (sending.open = false)}>Cancel</Button>
    <Button type="submit" form="send-form" disabled={!username.trim() || busy}>Send</Button>
  {/snippet}
</Dialog>
