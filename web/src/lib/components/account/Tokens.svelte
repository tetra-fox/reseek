<script lang="ts">
  import PlusIcon from "@lucide/svelte/icons/plus";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { onMount } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import { Busy } from "#lib/busy.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import { ago, dateTime } from "#lib/format.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";

  import NewToken from "./NewToken.svelte";

  type Listed = Schemas["ListedToken"];
  const keyOf = (t: Listed) => (t.source === "api" ? t.id : `config:${t.name}`);

  const rows = new Keyed<Listed>(keyOf);
  const loader = new Loader(async () => {
    const outcome = await call(() => api.GET("/api/v0/tokens"));
    if (outcome.ok) rows.reconcile(outcome.data);
    return outcome;
  });
  onMount(() => void loader.load());

  const busy = new Busy();
  let creating = $state(false);

  const revoke = (id: string, name: string) =>
    busy.run(async () => {
      const outcome = await call(() =>
        api.DELETE("/api/v0/tokens/{id}", { params: { path: { id } } })
      );
      if (outcome.ok) await loader.load();
      return outcome;
    }, `${name} revoked`);
</script>

<Card
  title="API tokens"
  description="For scripts and tools. A token cannot exceed the permissions of the account that created it.">
  {#snippet action()}
    <Button variant="outline" size="sm" onclick={() => (creating = true)}>
      <PlusIcon />New token
    </Button>
  {/snippet}
  <LoadError {loader} />
  {#snippet flush()}
    <div class="overflow-x-auto border-t">
      <table class="data-table w-full text-sm">
        <thead>
          <tr
            ><th>Name</th><th>Prefix</th><th>Role</th><th>Created</th><th>Expires</th><th
              >Last used</th
            ><th></th></tr>
        </thead>
        <tbody>
          {#each rows.rows as t (keyOf(t))}
            {#if t.source === "api"}
              <tr>
                <td>{t.name}</td>
                <td><code>slsk_{t.prefix}...</code></td>
                <td>{t.role}</td>
                <td>{dateTime(t.createdAt)}</td>
                <td>{t.expiresAt ? dateTime(t.expiresAt) : "never"}</td>
                <td
                  >{t.lastUsedAt
                    ? `${ago(t.lastUsedAt)} from ${t.lastUsedIp ?? "?"}`
                    : "never"}</td>
                <td class="text-right">
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Revoke ${t.name}`}
                    disabled={busy.running}
                    onclick={() => revoke(t.id, t.name)}>
                    <Trash2Icon />
                  </Button>
                </td>
              </tr>
            {:else}
              <!-- a key from the configuration is listed for reference; it is changed there -->
              <tr class="text-fg-muted">
                <td>{t.name} <Badge tone="outline">config</Badge></td>
                <td></td>
                <td>{t.role}</td>
                <td colspan="3">from {t.cidr}</td>
                <td></td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
  {/snippet}
</Card>

<NewToken bind:open={creating} oncreated={() => void loader.load()} />
