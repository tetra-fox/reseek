<script lang="ts">
  import InfoList from "#lib/components/system/InfoList.svelte";
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
  <Card title={principal.name} description={`${principal.role}, via ${via}`}>
    <InfoList>
      <dt>Subject</dt>
      <dd><code>{principal.subject}</code></dd>
      {#if principal.provider}
        <dt>Provider</dt>
        <dd>{principal.provider}</dd>
      {/if}
      {#if principal.session}
        <dt>Since</dt>
        <dd>{dateTime(principal.session.createdAt)}</dd>
        <dt>Session ends</dt>
        <dd>{dateTime(principal.session.expiresAt)}</dd>
      {/if}
    </InfoList>
  </Card>
{/if}
