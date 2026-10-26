<script lang="ts">
  import LogOutIcon from "@lucide/svelte/icons/log-out";
  import { onMount } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import { Busy } from "#lib/busy.svelte.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import { ago, dateTime } from "#lib/format.js";
  import { Keyed } from "#lib/keyed.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";

  const rows = new Keyed<Schemas["Session"]>((s) => s.id);
  const loader = new Loader(async () => {
    const outcome = await call(() => api.GET("/api/v0/sessions"));
    if (outcome.ok) rows.reconcile(outcome.data);
    return outcome;
  });
  onMount(() => void loader.load());

  const busy = new Busy();
  let everywhere = $state(false);
  const admin = $derived(session.can("Administrator"));

  const revoke = (id: string) =>
    busy.run(async () => {
      const outcome = await call(() =>
        api.DELETE("/api/v0/sessions/{id}", { params: { path: { id } } })
      );
      if (outcome.ok) await loader.load();
      return outcome;
    }, "Session signed out");

  const revokeOthers = () =>
    busy.run(async () => {
      const outcome = await call(() => api.DELETE("/api/v0/sessions"));
      if (outcome.ok) await loader.load();
      return outcome;
    }, "Every other session signed out");
</script>

<Card title="Sessions" description="The browsers signed in now">
  {#snippet action()}
    {#if admin}
      <Button
        variant="outline"
        size="sm"
        disabled={busy.running}
        onclick={() => (everywhere = true)}>
        Sign out everywhere
      </Button>
    {/if}
  {/snippet}
  <LoadError {loader} />
  {#snippet flush()}
    <div class="overflow-x-auto border-t">
      <table class="data-table w-full text-sm">
        <thead>
          <tr
            ><th>Who</th><th>Role</th><th>Signed in</th><th>Last used</th><th>From</th><th></th
            ></tr>
        </thead>
        <tbody>
          {#each rows.rows as s (s.id)}
            <tr>
              <td>
                {s.name}
                {#if s.current}<Badge tone="accent">this browser</Badge>{/if}
              </td>
              <td>{s.role}</td>
              <td>{dateTime(s.createdAt)}</td>
              <td>{ago(s.lastSeenAt)}</td>
              <td class="max-w-64 truncate" title={s.userAgent ?? undefined}>{s.ip ?? "?"}</td>
              <td class="text-right">
                {#if !s.current}
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Sign out ${s.name}'s session`}
                    disabled={busy.running}
                    onclick={() => revoke(s.id)}>
                    <LogOutIcon />
                  </Button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/snippet}
</Card>

<ConfirmDialog
  bind:open={everywhere}
  title="Sign out everywhere"
  body="Every session but this one ends now; each of those browsers has to log in again."
  confirm="Sign out everywhere"
  danger
  onconfirm={revokeOthers} />
