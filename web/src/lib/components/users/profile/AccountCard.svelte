<script lang="ts">
  import KeyRoundIcon from "@lucide/svelte/icons/key-round";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { href } from "#lib/nav.js";
  import { SOULSEEK_SETTINGS } from "#lib/reachability.js";
  import { call } from "#lib/request.js";

  // the Soulseek account this daemon logs in with. The name is a setting; the password changes at
  // the server, so it takes being logged in, and is typed twice since nothing can show it again
  type Props = {
    name: string;
    /** the password can be changed from here at all */
    allowed: boolean;
    /** why it can't, or what comes of changing it */
    notice: string | null;
  };

  let { name, allowed, notice }: Props = $props();

  const online = $derived(app.state?.server.isLoggedIn === true);
  let open = $state(false);
  let password = $state("");
  let repeated = $state("");
  let error = $state("");
  let changing = $state(false);

  const mismatch = $derived(!!repeated && password !== repeated);
  const ready = $derived(!!password.trim() && password === repeated);

  const start = () => {
    password = "";
    repeated = "";
    error = "";
    open = true;
  };

  const change = async (e: SubmitEvent) => {
    e.preventDefault();
    if (!ready || changing) return;
    changing = true;
    const outcome = await call(() =>
      api.PUT("/api/v0/application/password", { body: { password } })
    );
    changing = false;
    if (!outcome.ok) {
      error = outcome.error;
      return;
    }
    open = false;
    toast("Password changed");
  };
</script>

<Card title="Account">
  <dl class="flex flex-col gap-1 text-sm">
    <dt class="text-xs font-medium text-fg-muted">Soulseek username</dt>
    <dd class="min-w-0"><Username {name} /></dd>
  </dl>
  <div class="flex flex-wrap gap-inline">
    {#if allowed}
      <Button variant="outline" size="sm" disabled={!online} onclick={start}>
        <KeyRoundIcon />Change password
      </Button>
    {/if}
    <Button variant="ghost" size="sm" href={href(SOULSEEK_SETTINGS)}>Soulseek settings</Button>
  </div>
  {#if allowed && !online}
    <p class="text-xs text-fg-muted">Connect to the Soulseek server to change the password</p>
  {/if}
  {#if notice}<p class="text-xs text-fg-muted">{notice}</p>{/if}
</Card>

<Dialog
  bind:open
  busy={changing}
  title="Change password"
  description="This takes effect at once. Other clients on this account need the new password at their next login.">
  <form id="change-password" class="flex flex-col gap-group" onsubmit={change}>
    <!-- for password managers, which pair a new password with its account -->
    <input type="text" class="hidden" autocomplete="username" value={name} readonly />
    <label class="flex flex-col gap-1 text-xs font-medium text-fg-muted">
      New password
      <Input type="password" autocomplete="new-password" bind:value={password} required />
    </label>
    <label class="flex flex-col gap-1 text-xs font-medium text-fg-muted">
      New password again
      <Input
        type="password"
        autocomplete="new-password"
        aria-invalid={mismatch}
        bind:value={repeated}
        required />
    </label>
    {#if mismatch}<p class="text-xs text-danger">The passwords do not match</p>{/if}
    {#if error}<Notice kind="error">{error}</Notice>{/if}
  </form>
  {#snippet footer()}
    <Button variant="ghost" disabled={changing} onclick={() => (open = false)}>Cancel</Button>
    <Button type="submit" form="change-password" disabled={!ready || changing}>
      Change password
    </Button>
  {/snippet}
</Dialog>
