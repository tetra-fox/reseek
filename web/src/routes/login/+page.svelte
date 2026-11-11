<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import LogInIcon from "@lucide/svelte/icons/log-in";
  import { onMount } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import Field from "#lib/components/Field.svelte";
  import Form from "#lib/components/Form.svelte";
  import Logo from "#lib/components/shell/Logo.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { href, route, safeReturn, title } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";

  const query = $derived(route(page.url).query);
  // where the session ran out, a route of this app only
  const back = $derived(safeReturn(query.get("to")));
  // why the identity provider's login came back without a session
  const refused = $derived(query.get("error"));

  let providers = $state.raw<Schemas["Providers"] | null>(null);
  let loadError = $state("");
  let username = $state("");
  let password = $state("");
  let error = $state("");
  let submitting = $state(false);

  /** The identity provider's login, coming back to the route the user was on */
  const oidcLogin = (provider: Schemas["OidcProvider"]) =>
    `${provider.loginUrl}?redirect=${encodeURIComponent(`${location.pathname}${href(back)}`)}`;

  onMount(async () => {
    const outcome = await call(() => api.GET("/api/v0/session/providers"));
    if (!outcome.ok) {
      loadError = outcome.error;
      return;
    }
    // a trusted network needs no login, and a provider set to log in by itself is gone to
    if (outcome.data.trustedNetwork) return void goto(href(back), { replace: true });
    const auto = outcome.data.oidc.find((p) => p.autoLogin);
    if (auto && !refused) return void window.location.assign(oidcLogin(auto));
    providers = outcome.data;
  });

  // the username takes focus when the form shows, and again once a login attempt is over:
  // disabling the fields while it runs drops the focus
  const focusWhenReady = (input: HTMLInputElement) => {
    if (!submitting) input.focus();
  };

  const ready = $derived(username.trim() !== "" && password.trim() !== "" && !submitting);

  const login = async (event: SubmitEvent) => {
    event.preventDefault();
    if (!ready) return;
    submitting = true;
    error = "";
    const outcome = await session.login(username, password);
    submitting = false;
    if (outcome.ok) await goto(href(back), { replace: true });
    else error = outcome.error;
  };
</script>

<svelte:head>
  <title>{title("/login")}</title>
</svelte:head>

<div class="flex min-h-svh flex-col items-center justify-center p-6">
  <div
    class="flex w-full max-w-sm flex-col gap-6 rounded-card border bg-surface p-6 shadow-raised sm:p-8">
    <div class="flex flex-col items-center gap-3 text-center">
      <Logo class="size-11" />
      <div class="flex flex-col gap-0.5">
        <h1 class="text-xl font-medium">reseek</h1>
        <p class="text-sm text-fg-muted">Soulseek daemon</p>
      </div>
    </div>
    {#if refused}
      <Notice kind="error">Could not log in: {refused}</Notice>
    {/if}
    {#if loadError}
      <Notice kind="error">{loadError}</Notice>
    {:else if !providers}
      <div class="flex justify-center"><Spinner class="size-6" /></div>
    {:else}
      {#if providers.oidc.length}
        <div class="flex flex-col gap-2">
          {#each providers.oidc as provider (provider.name)}
            <Button variant="outline" class="w-full" href={oidcLogin(provider)}>
              Log in with {provider.displayName}
            </Button>
          {/each}
        </div>
      {/if}
      {#if providers.local.enabled}
        {#if providers.oidc.length}
          <div class="flex items-center gap-3 text-xs text-fg-muted">
            <span class="h-px flex-1 bg-border"></span>or<span class="h-px flex-1 bg-border"></span>
          </div>
        {/if}
        <Form layout="stacked" class="flex flex-col gap-4" onsubmit={login}>
          <Field label="Username" for="username">
            <Input
              id="username"
              autocomplete="username"
              disabled={submitting}
              bind:value={username}
              {@attach focusWhenReady} />
          </Field>
          <Field label="Password" for="password">
            <Input
              id="password"
              type="password"
              autocomplete="current-password"
              disabled={submitting}
              bind:value={password} />
          </Field>
          <Button type="submit" class="w-full" disabled={!ready}>
            {#if submitting}<Spinner class="size-4" label="Logging in" />{:else}<LogInIcon />{/if}
            Log in
          </Button>
          {#if error}
            <p class="text-sm text-danger" role="alert">{error}</p>
          {/if}
        </Form>
      {:else if !providers.oidc.length}
        <Notice kind="warning">
          No login method is enabled on this server. Contact its administrator.
        </Notice>
      {/if}
    {/if}
  </div>
</div>
