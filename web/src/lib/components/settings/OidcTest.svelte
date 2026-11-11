<script lang="ts">
  import { api, type Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { call } from "#lib/request.js";

  // a discovery check for each configured provider, its outcome shown under it
  let { names }: { names: string[] } = $props();

  const busy = new Busy();
  let results = $state<Record<string, Schemas["OidcTestResult"]>>({});

  const test = (name: string) =>
    busy.run(async () => {
      const outcome = await call(() =>
        api.POST("/api/v0/auth/oidc/{name}/test", { params: { path: { name } } })
      );
      if (outcome.ok) results[name] = outcome.data;
      return outcome;
    });
</script>

{#each names as name (name)}
  {@const result = results[name]}
  <div class="flex flex-col gap-1 text-xs">
    <div class="flex flex-wrap items-center gap-inline">
      <Button
        variant="outline"
        size="sm"
        class="max-w-full"
        disabled={busy.running}
        onclick={() => test(name)}>
        <span class="truncate">Test discovery: {name}</span>
      </Button>
      {#if result}
        <span
          class={["min-w-0 wrap-anywhere", result.ok ? "text-success-soft-fg" : "text-danger"]}
          role="status">
          {result.ok ? "Passed" : `Failed: ${result.error ?? "unknown error"}`}
        </span>
      {/if}
    </div>
    {#if result?.issuer}<p class="wrap-anywhere text-fg-muted">
        Issuer <code>{result.issuer}</code>
      </p>{/if}
    {#if result?.endpoints}
      {@const e = result.endpoints}
      <ul class="font-mono wrap-anywhere text-fg-muted">
        <li>authorization {e.authorization}</li>
        <li>token {e.token}</li>
        <li>jwks {e.jwks}</li>
        {#if e.userinfo}<li>userinfo {e.userinfo}</li>{/if}
        {#if e.endSession}<li>end session {e.endSession}</li>{/if}
      </ul>
    {/if}
  </div>
{/each}
