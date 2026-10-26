<script lang="ts">
  import Card from "#lib/components/ui/Card.svelte";
  import { dateTime } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";

  const principal = $derived(session.principal);
  const via = $derived.by(() => {
    const v = principal?.via;
    if (!v) return "";
    if (v.kind === "local") return "password login";
    if (v.kind === "oidc") return `OpenID Connect (${v.provider})`;
    if (v.kind === "proxy") return "reverse proxy";
    if (v.kind === "apiToken") return "API token";
    if (v.kind === "configApiKey") return `API key ${v.name}`;
    return `trusted network ${v.cidr}`;
  });
</script>

<!-- who this browser is signed in as -->
{#if principal}
  <Card title={principal.name} description={`${principal.role}, by ${via}`}>
    <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm">
      <dt class="text-fg-muted">Subject</dt>
      <dd><code>{principal.subject}</code></dd>
      {#if principal.provider}
        <dt class="text-fg-muted">Provider</dt>
        <dd>{principal.provider}</dd>
      {/if}
      {#if principal.session}
        <dt class="text-fg-muted">Since</dt>
        <dd>{dateTime(principal.session.createdAt)}</dd>
        <dt class="text-fg-muted">Session ends</dt>
        <dd>{dateTime(principal.session.expiresAt)}</dd>
      {/if}
    </dl>
  </Card>
{/if}
