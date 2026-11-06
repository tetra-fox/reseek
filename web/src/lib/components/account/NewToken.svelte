<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import CopyIcon from "@lucide/svelte/icons/copy";

  import { api, root, type Schemas } from "#lib/api/client.js";
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { CopyButton } from "#lib/feedback/clipboard.svelte.js";
  import { call } from "#lib/request.js";
  import { type Role, session } from "#lib/session.svelte.js";

  let { open = $bindable(), oncreated }: { open: boolean; oncreated: () => void } = $props();

  const ROLES: Role[] = ["ReadOnly", "ReadWrite", "Administrator"];
  const EXPIRIES = [
    { value: "7d", label: "A week" },
    { value: "30d", label: "30 days" },
    { value: "90d", label: "90 days" },
    { value: "365d", label: "A year" },
    { value: null, label: "Never" }
  ];

  // a token may do at most what its creator may
  const roles = $derived(ROLES.filter((r) => session.can(r)));
  let name = $state("");
  let role = $state<Role>("ReadOnly");
  let expiresIn = $state<string | null>("90d");
  let cidr = $state("");
  let created = $state.raw<Schemas["CreatedToken"] | null>(null);

  const busy = new Busy();
  const copying = new CopyButton();

  const create = (event: SubmitEvent) => {
    event.preventDefault();
    void busy.run(async () => {
      const outcome = await call(() =>
        api.POST("/api/v0/tokens", {
          body: { name: name.trim(), role, expiresIn, cidr: cidr.trim() || null }
        })
      );
      if (outcome.ok) {
        created = outcome.data;
        oncreated();
      }
      return outcome;
    });
  };

  const close = (next: boolean) => {
    open = next;
    if (!next) {
      // the secret is shown this once; closing forgets it
      created = null;
      name = "";
      cidr = "";
    }
  };
</script>

<Dialog bind:open={() => open, close} title="New API token" busy={busy.running}>
  {#if created}
    <p class="text-sm">Copy the secret now. It is not shown again.</p>
    <div class="flex items-center gap-inline">
      <code
        class="min-w-0 flex-1 rounded-control bg-surface-muted px-2 py-1.5 text-xs wrap-anywhere">
        {created.secret}
      </code>
      <Button
        variant="outline"
        size="icon-sm"
        aria-label="Copy the secret"
        onclick={() => copying.copy(created!.secret)}>
        {#if copying.copied}<CheckIcon />{:else}<CopyIcon />{/if}
      </Button>
    </div>
    <p class="text-xs text-fg-muted">Use it as a bearer token, for example:</p>
    <code class="rounded-control bg-surface-muted px-2 py-1.5 text-xs wrap-anywhere">
      curl -H 'Authorization: Bearer {created.secret}' {root()}/api/v0/application
    </code>
  {:else}
    <Form layout="stacked" class="flex flex-col gap-4" onsubmit={create}>
      <Field label="Name" for="token-name">
        <Input
          id="token-name"
          bind:value={name}
          required
          maxlength={64}
          placeholder="backup script" />
      </Field>
      <Field label="Role">
        <Select
          aria-label="Role"
          bind:value={role}
          options={roles.map((r) => ({ value: r, label: r }))} />
      </Field>
      <Field label="Expires">
        <Select aria-label="Expires" bind:value={expiresIn} options={EXPIRIES} />
      </Field>
      <Field label="Only from" for="token-cidr" hint="Comma-separated networks, or empty for any">
        <Input id="token-cidr" bind:value={cidr} placeholder="192.168.1.0/24" />
      </Field>
      <div class="flex justify-end">
        <Button type="submit" disabled={busy.running}>Create</Button>
      </div>
    </Form>
  {/if}
</Dialog>
