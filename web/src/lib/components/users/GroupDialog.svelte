<script lang="ts">
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { plural } from "#lib/format.js";
  import { assignGroup, userGroups } from "#lib/users/actions.js";
  import { users } from "#lib/users/users.svelte.js";

  // puts users in one of the user-defined upload groups, or takes them out
  let { open = $bindable(), usernames }: { open: boolean; usernames: string[] } = $props();

  let group = $state<string | null>(null);
  let error = $state("");
  let sending = $state(false);

  // each opening starts without the last one's error
  $effect(() => {
    if (open) error = "";
  });

  const save = async () => {
    sending = true;
    const to = group;
    const failed: string[] = [];
    for (const name of usernames) {
      const outcome = await assignGroup(name, to);
      if (outcome.ok) void users.load(name, { fresh: true });
      else failed.push(`${name}: ${outcome.error}`);
    }
    sending = false;
    if (failed.length) error = failed.join("; ");
    else {
      open = false;
      toast(to ? `Moved to group ${to}` : "Removed from the upload group");
    }
  };
</script>

<Dialog
  bind:open
  busy={sending}
  title={usernames.length === 1
    ? `Upload group for ${usernames[0]}`
    : `Upload group for ${plural(usernames.length, "user", "users")}`}
  description="A group set in the configuration overrides this one">
  <Form layout="stacked">
    <Field label="Upload group" for="assign-group">
      <Select
        id="assign-group"
        bind:value={group}
        options={[
          { value: null, label: "None" },
          ...userGroups().map((g) => ({ value: g as string | null, label: g }))
        ]} />
    </Field>
  </Form>
  {#if error}<Notice kind="error">{error}</Notice>{/if}
  {#snippet footer()}
    <Button variant="ghost" disabled={sending} onclick={() => (open = false)}>Cancel</Button>
    <Button disabled={sending} onclick={() => void save()}>Save</Button>
  {/snippet}
</Dialog>
