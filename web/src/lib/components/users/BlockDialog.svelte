<script lang="ts">
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { plural } from "#lib/format.js";
  import {
    addressesIn,
    addressOf,
    ALSO_SCOPES,
    block,
    type BlockScope,
    blocking,
    CHAT_SCOPES,
    defaultScopes,
    STOP_SCOPES
  } from "#lib/users/block.svelte.js";

  // the page's one block dialog: reason, how long, what the block stops, and for one user the
  // addresses blocked with them
  const DURATIONS = [
    { value: "", label: "Until unblocked" },
    { value: "1h", label: "1 hour" },
    { value: "1d", label: "1 day" },
    { value: "7d", label: "7 days" },
    { value: "30d", label: "30 days" },
    { value: "custom", label: "Custom" }
  ];

  let reason = $state("");
  let duration = $state("");
  let custom = $state("");
  let scopes = $state<BlockScope[]>([]);
  let addresses = $state("");
  /** the user's address is being asked for */
  let finding = $state(false);
  let error = $state("");
  let sending = $state(false);

  const names = $derived(blocking.asked?.usernames ?? []);
  const title = $derived(names.length === 1 ? names[0] : plural(names.length, "user", "users"));

  // each opening starts from the defaults, with what the caller adds ticked
  $effect(() => {
    const asked = blocking.asked;
    if (!blocking.open || !asked) return;
    reason = "";
    duration = "";
    custom = "";
    error = "";
    scopes = [...new Set([...defaultScopes(), ...(asked.add ?? [])])];
    addresses = "";
  });

  const tick = (scope: BlockScope, on: boolean) => {
    scopes = on ? [...scopes, scope] : scopes.filter((s) => s !== scope);
  };

  const listed = $derived(addressesIn(addresses));
  const chatOnly = $derived(
    scopes.length === CHAT_SCOPES.length && CHAT_SCOPES.every((s) => scopes.includes(s))
  );
  /** Adds the address the server gives for the one user, said when there is none */
  const addTheirs = async () => {
    const asked = blocking.asked;
    finding = true;
    const address = await addressOf(names[0]);
    finding = false;
    if (blocking.asked !== asked) return;
    if (!address) error = `No address known for ${names[0]}, who may be offline`;
    else if (!listed.includes(address)) addresses = [...listed, address].join(" ");
  };

  const expiresIn = $derived(duration === "custom" ? custom.trim() : duration);
  const valid = $derived(scopes.length > 0 && (duration !== "custom" || !!expiresIn));

  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    if (!valid || sending) return;
    sending = true;
    const who = names;
    const blocked = title;
    const request = {
      reason: reason.trim(),
      scopes,
      addresses: who.length === 1 ? listed : [],
      expiresIn: expiresIn || null,
      cancelTransfers: true
    };
    const failed: string[] = [];
    for (const name of who) {
      const outcome = await block(name, request);
      if (!outcome.ok) failed.push(who.length === 1 ? outcome.error : `${name}: ${outcome.error}`);
    }
    sending = false;
    if (failed.length) {
      error = failed.join("; ");
      return;
    }
    blocking.open = false;
    toast(`${blocked} ${who.length === 1 ? "is" : "are"} blocked`);
  };
</script>

{#snippet scopeBox(scope: BlockScope, label: string)}
  <label class="flex items-center gap-2 text-sm">
    <Checkbox checked={scopes.includes(scope)} onCheckedChange={(on) => tick(scope, on)} />
    {label}
  </label>
{/snippet}

<Dialog bind:open={blocking.open} title={`Block ${title}`} busy={sending}>
  <Form id="block-form" layout="stacked" class="flex flex-col gap-group" onsubmit={submit}>
    <Field label="Reason" for="block-reason" hint="Not shown to the user">
      <Input id="block-reason" maxlength={500} bind:value={reason} autocomplete="off" />
    </Field>
    <Field label="Duration" for="block-duration">
      <Select id="block-duration" bind:value={duration} options={DURATIONS} />
    </Field>
    {#if duration === "custom"}
      <Field label="Custom duration" for="block-custom">
        <Input id="block-custom" placeholder="For example 12h or 3d" required bind:value={custom} />
      </Field>
    {/if}
    <fieldset class="flex flex-col gap-1.5">
      <legend class="mb-1 text-sm font-medium">What to block</legend>
      {#each STOP_SCOPES as { scope, label } (scope)}
        {@render scopeBox(scope, label)}
      {/each}
      <Button
        variant="link"
        class="self-start text-xs"
        disabled={chatOnly}
        onclick={() => (scopes = [...CHAT_SCOPES])}>Only hide their messages</Button>
    </fieldset>
    <fieldset class="flex flex-col gap-1.5">
      <legend class="mb-1 text-sm font-medium">Also</legend>
      {#each ALSO_SCOPES as { scope, label } (scope)}
        {@render scopeBox(scope, label)}
      {/each}
    </fieldset>
    {#if names.length === 1}
      <Field
        label="Addresses"
        for="block-addresses"
        hint="IP addresses or networks such as 203.0.113.0/24, separated by spaces. Anyone connecting from them is blocked the same way.">
        <Input
          id="block-addresses"
          class="font-mono"
          placeholder="Optional"
          autocomplete="off"
          bind:value={addresses} />
      </Field>
      <Button
        variant="link"
        class="-mt-2 self-start text-xs"
        disabled={finding}
        onclick={() => void addTheirs()}>Add their current address</Button>
    {/if}
    {#if !scopes.length}
      <p class="text-xs text-danger">Select at least one item to block</p>
    {/if}
    {#if error}<Notice kind="error">{error}</Notice>{/if}
  </Form>
  {#snippet footer()}
    <Button variant="ghost" disabled={sending} onclick={() => (blocking.open = false)}
      >Cancel</Button>
    <Button type="submit" form="block-form" variant="danger" disabled={!valid || sending}
      >Block</Button>
  {/snippet}
</Dialog>
