<script lang="ts">
  import { goto } from "$app/navigation";

  import { api, byUser } from "#lib/api/client.js";
  import { NOT_A_USER, SERVER } from "#lib/chat/notices.js";
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import * as Suggest from "#lib/components/ui/suggest/suggest.js";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import UserEntry from "#lib/components/users/UserEntry.svelte";
  import { chatHref } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { COMPOSER } from "#lib/ui/bindings.js";
  import { isPressed } from "#lib/ui/keys.js";
  import { USER_ACTIONS } from "#lib/users/actions.js";
  import { UserFinder, userOf, userValue } from "#lib/users/finder.svelte.js";

  import LengthCount, { maxLength, nearMax } from "./LengthCount.svelte";

  // a first message to someone, with the users the directory knows offered as the name is typed.
  // Names are exact, so Enter keeps the name as typed unless the keys moved to one of them
  let { open = $bindable(), username: start = "" }: { open: boolean; username?: string } = $props();

  /** the entry that keeps the name as typed, first when the directory does not hold it */
  const TYPED = "typed";

  let username = $state("");
  let message = $state("");
  let error = $state("");
  let sending = $state(false);
  let suggesting = $state(false);
  let messageBox = $state<HTMLTextAreaElement | null>(null);

  // a name is taken as typed, edge spaces and all; only a blank one is no name
  const name = $derived(username.trim() ? username : "");
  const refused = $derived(name === SERVER);
  const finder = new UserFinder(() => name);
  const problem = $derived(refused ? NOT_A_USER : error);
  const max = $derived(maxLength());
  const length = $derived([...message.trim()].length);

  $effect(() => {
    if (!open) return;
    username = start;
    message = "";
    error = "";
  });

  const choose = (value: string) => {
    if (value !== TYPED) username = userOf(value);
    messageBox?.focus();
  };

  const send = async (e: SubmitEvent) => {
    e.preventDefault();
    const text = message.trim();
    if (!name || refused || !text || length > max || sending) return;
    const to = name;
    sending = true;
    const outcome = await call(() =>
      api.POST("/api/v0/conversations/{username}", {
        params: byUser(to),
        body: text
      })
    );
    sending = false;
    if (!outcome.ok) return void (error = outcome.error);
    open = false;
    void goto(chatHref(to));
  };
</script>

<Dialog bind:open title="New message" busy={sending}>
  <Form id="new-message" layout="stacked" class="flex flex-col gap-group" onsubmit={send}>
    <Field label="Username" for="new-message-user">
      <Suggest.Box
        id="new-message-user"
        bind:value={username}
        bind:open={suggesting}
        first={refused ? null : finder.unknown ? TYPED : finder.first}
        listed={!refused && (finder.unknown || !!finder.matches.length)}
        onchoose={choose}
        autocomplete="off"
        required
        autofocus>
        <!-- entries made anew move the highlight back to the first when the users arrive -->
        {#key finder.found}
          {#if finder.unknown}
            <Suggest.Item value={TYPED} icon={USER_ACTIONS.message.icon}>
              {USER_ACTIONS.message.of(name)}
            </Suggest.Item>
          {/if}
          {#each finder.matches as u (u.username)}
            <Suggest.Item value={userValue(u.username)}>
              <UserEntry user={{ name: u.username, summary: u }} text={name} />
            </Suggest.Item>
          {/each}
        {/key}
      </Suggest.Box>
    </Field>
    <Field label="Message" for="new-message-text">
      <!-- a private message is one line: Enter sends it and Shift+Enter breaks none, as in a
           conversation -->
      <Textarea
        id="new-message-text"
        bind:ref={messageBox}
        class="field-sizing-content max-h-48 min-h-10 text-sm"
        autocomplete="off"
        required
        bind:value={message}
        onkeydown={(e) => {
          if (e.isComposing) return;
          if (isPressed(COMPOSER.send, e)) {
            e.preventDefault();
            e.currentTarget.form?.requestSubmit();
          } else if (isPressed(COMPOSER.newLine, e)) e.preventDefault();
        }} />
      {#if nearMax(length, max)}<p class="flex text-xs"><LengthCount {length} {max} /></p>{/if}
    </Field>
    {#if problem}<Notice kind="error">{problem}</Notice>{/if}
  </Form>
  {#snippet footer()}
    <Button variant="ghost" disabled={sending} onclick={() => (open = false)}>Cancel</Button>
    <Button
      type="submit"
      form="new-message"
      disabled={!name || refused || !message.trim() || length > max || sending}>Send</Button>
  {/snippet}
</Dialog>
