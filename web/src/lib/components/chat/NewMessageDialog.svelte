<script lang="ts">
  import { goto } from "$app/navigation";

  import { api } from "#lib/api/client.js";
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import { chatHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";

  // a first message to someone, with the users the directory knows offered as the name is typed
  let { open = $bindable(), username: start = "" }: { open: boolean; username?: string } = $props();

  let username = $state("");
  let message = $state("");
  let error = $state("");
  let sending = $state(false);
  let known = $state<string[]>([]);

  $effect(() => {
    if (!open) return;
    username = start;
    message = "";
    error = "";
  });

  // the names the directory knows that hold what is typed
  $effect(() => {
    const q = username.trim();
    if (!q) return void (known = []);
    const timer = setTimeout(async () => {
      const outcome = await call(() =>
        api.GET("/api/v0/users", { params: { query: { q, limit: 8, sort: "username" } } })
      );
      known = outcome.ok ? outcome.data.users.map((u) => u.username) : [];
    }, 200);
    return () => clearTimeout(timer);
  });

  const send = async (e: SubmitEvent) => {
    e.preventDefault();
    const to = username.trim();
    const text = message.trim();
    if (!to || !text || sending) return;
    sending = true;
    const outcome = await call(() =>
      api.POST("/api/v0/conversations/{username}", {
        params: { path: { username: to }, query: { username: to } },
        body: text
      })
    );
    sending = false;
    if (!outcome.ok) return void (error = outcome.error);
    open = false;
    void goto(chatHref(to));
  };
</script>

<Dialog bind:open title="New message">
  <Form id="new-message" layout="stacked" class="flex flex-col gap-group" onsubmit={send}>
    <Field label="Username" for="new-message-user">
      <!-- svelte-ignore a11y_autofocus -->
      <Input
        id="new-message-user"
        list="new-message-known"
        autocomplete="off"
        required
        autofocus
        bind:value={username} />
      <datalist id="new-message-known">
        {#each known as name (name)}<option value={name}></option>{/each}
      </datalist>
    </Field>
    <Field label="Message" for="new-message-text">
      <Input id="new-message-text" autocomplete="off" required bind:value={message} />
    </Field>
    {#if error}<Notice kind="error">{error}</Notice>{/if}
  </Form>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
    <Button
      type="submit"
      form="new-message"
      disabled={!username.trim() || !message.trim() || sending}>Send</Button>
  {/snippet}
</Dialog>
