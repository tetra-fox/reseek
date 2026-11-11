<script lang="ts">
  import { api, byUser } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { plural } from "#lib/format.js";
  import { call } from "#lib/request.js";

  // gives a user whole days of our Soulseek privileges, at most the days we have left
  type Props = { username: string; open: boolean };
  let { username, open = $bindable() }: Props = $props();

  const DAY = 86_400;
  const daysLeft = $derived(
    Math.floor((app.state?.user.privileges.privilegesRemaining ?? 0) / DAY)
  );

  let typed = $state("1");
  let error = $state("");
  let sending = $state(false);

  // each opening starts over and asks the server for our privileges, since the state's copy can
  // be hours old; the state frame brings the answer
  $effect(() => {
    if (!open) return;
    typed = "1";
    error = "";
    void call(() => api.GET("/api/v0/application/privileges")).then((outcome) => {
      if (!outcome.ok) error = outcome.error;
    });
  });

  const days = $derived(/^\d+$/.test(typed.trim()) ? Number(typed.trim()) : 0);
  const valid = $derived(days >= 1 && days <= daysLeft);

  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    if (!valid || sending) return;
    sending = true;
    const given = days;
    const outcome = await call(() =>
      api.POST("/api/v0/users/{username}/privileges", {
        params: byUser(username),
        body: { days: given }
      })
    );
    sending = false;
    if (!outcome.ok) {
      error = outcome.error;
      return;
    }
    open = false;
    toast(`Gave ${username} ${plural(given, "day", "days")} of privileges`);
  };
</script>

<Dialog
  bind:open
  busy={sending}
  title="Give privileges"
  description="Moves them ahead in other users' queues, using days of your own privileges. A refusal comes as a private message.">
  {#if daysLeft < 1}
    <Notice kind="info">You have no whole days of privileges to give</Notice>
  {:else}
    <Form id="give-privileges" layout="stacked" class="flex flex-col gap-group" onsubmit={submit}>
      <Field
        label="Days"
        for="give-privileges-days"
        hint={`You have ${plural(daysLeft, "whole day", "whole days")} left`}>
        <Input
          id="give-privileges-days"
          class="tabular-nums"
          inputmode="numeric"
          autocomplete="off"
          required
          aria-invalid={typed.trim() !== "" && !valid}
          bind:value={typed} />
      </Field>
      {#if error}<Notice kind="error">{error}</Notice>{/if}
    </Form>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" disabled={sending} onclick={() => (open = false)}>Cancel</Button>
    {#if daysLeft >= 1}
      <Button type="submit" form="give-privileges" disabled={!valid || sending}>
        Give {plural(days, "day", "days")}
      </Button>
    {/if}
  {/snippet}
</Dialog>
