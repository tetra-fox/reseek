<script lang="ts">
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import { toast } from "#lib/toast.svelte.js";

  import {
    ALSO_SCOPES,
    block,
    type BlockScope,
    blocking,
    defaultScopes,
    STOP_SCOPES
  } from "./block.svelte";

  // the page's one block dialog: reason, how long, and what the block stops
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
  let error = $state("");
  let sending = $state(false);

  const names = $derived(blocking.asked?.usernames ?? []);
  const title = $derived(names.length === 1 ? names[0] : `${names.length} users`);

  // each opening starts from the defaults, with what the caller adds ticked
  $effect(() => {
    const asked = blocking.asked;
    if (!blocking.open || !asked) return;
    reason = "";
    duration = "";
    custom = "";
    error = "";
    scopes = [...new Set([...defaultScopes(), ...(asked.add ?? [])])];
  });

  const tick = (scope: BlockScope, on: boolean) => {
    scopes = on ? [...scopes, scope] : scopes.filter((s) => s !== scope);
  };

  const expiresIn = $derived(duration === "custom" ? custom.trim() : duration);
  const valid = $derived(scopes.length > 0 && (duration !== "custom" || !!expiresIn));

  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    if (!valid || sending) return;
    sending = true;
    const failed: string[] = [];
    for (const name of names) {
      const outcome = await block(name, {
        reason: reason.trim(),
        scopes,
        expiresIn: expiresIn || null,
        cancelTransfers: true
      });
      if (!outcome.ok)
        failed.push(names.length === 1 ? outcome.error : `${name}: ${outcome.error}`);
    }
    sending = false;
    if (failed.length) {
      error = failed.join("; ");
      return;
    }
    blocking.open = false;
    toast(`${title} ${names.length === 1 ? "is" : "are"} blocked`);
  };
</script>

{#snippet scopeBox(scope: BlockScope, label: string)}
  <label class="flex items-center gap-2 text-sm">
    <Checkbox checked={scopes.includes(scope)} onCheckedChange={(on) => tick(scope, on)} />
    {label}
  </label>
{/snippet}

<Dialog bind:open={blocking.open} title={`Block ${title}`}>
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
    </fieldset>
    <fieldset class="flex flex-col gap-1.5">
      <legend class="mb-1 text-sm font-medium">Also</legend>
      {#each ALSO_SCOPES as { scope, label } (scope)}
        {@render scopeBox(scope, label)}
      {/each}
    </fieldset>
    {#if !scopes.length}
      <p class="text-xs text-danger">Select at least one item to block.</p>
    {/if}
    {#if error}<Notice kind="error">{error}</Notice>{/if}
  </Form>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (blocking.open = false)}>Cancel</Button>
    <Button type="submit" form="block-form" variant="danger" disabled={!valid || sending}>
      Block {title}
    </Button>
  {/snippet}
</Dialog>
