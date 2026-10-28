<script lang="ts">
  import Trash2Icon from "@lucide/svelte/icons/trash-2";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { Busy } from "#lib/busy.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import type { ConfigView } from "#lib/config/fields.js";
  import { valueAt } from "#lib/config/sections.js";
  import { dateTime } from "#lib/format.js";
  import { call } from "#lib/request.js";
  import { UNTIL_RESTART } from "#lib/wording.js";

  // what the configuration as a whole says: its files, what was ignored or failed, what waits on
  // a reconnect or a restart, and the runtime overrides in effect
  let { view, onchanged }: { view: ConfigView; onchanged: () => Promise<void> } = $props();

  const busy = new Busy();
  const overrides = $derived(Object.entries(view.overrides));
  const remote = $derived(valueAt(view.values, "remote_configuration") === true);
  let restarting = $state(false);

  const reconnect = () =>
    busy.run(async () => {
      const down = await call(() =>
        api.DELETE("/api/v0/server", { body: "reconnecting for new settings" })
      );
      if (!down.ok) return down;
      return call(() => api.PUT("/api/v0/server"));
    }, "Reconnecting");

  const restart = () => busy.run(() => call(() => api.PUT("/api/v0/application")), "Restarting");

  const remove = (key: string) =>
    busy.run(async () => {
      const outcome = await call(() =>
        api.DELETE("/api/v0/options/overrides/{key}", { params: { path: { key } } })
      );
      if (outcome.ok) await onchanged();
      return outcome;
    }, `${key} is back to its configured value`);
</script>

{#if !remote}
  <Notice kind="warning">
    Changes from here need <code>remote_configuration</code> on; until then the settings are only shown.
  </Notice>
{/if}
{#if view.loadError}
  <Notice kind="error"
    >The last reload failed, the values before it still run: {view.loadError}</Notice>
{/if}
{#each view.warnings as warning (warning)}
  <Notice kind="warning">{warning}</Notice>
{/each}
{#if view.pendingRestart.length || app.state?.pendingRestart}
  <Notice kind="warning">
    A restart applies {view.pendingRestart.join(", ") || "a change"}.
    {#snippet action()}
      <Button
        variant="outline"
        size="sm"
        disabled={busy.running}
        onclick={() => (restarting = true)}>
        Restart now
      </Button>
    {/snippet}
  </Notice>
{/if}
{#if view.pendingReconnect.length || app.state?.pendingReconnect}
  <Notice kind="warning">
    A reconnect to the Soulseek server applies {view.pendingReconnect.join(", ") || "a change"}.
    {#snippet action()}
      <Button variant="outline" size="sm" disabled={busy.running} onclick={reconnect}>
        Reconnect now
      </Button>
    {/snippet}
  </Notice>
{/if}

<Card title="Files" description={view.managedBy ? `Managed by ${view.managedBy}` : undefined}>
  <ul class="flex flex-col gap-2 text-sm">
    {#each view.files as file (file.name)}
      <li class="flex flex-wrap items-center gap-inline">
        <span class="font-medium">{file.name}</span>
        <code class="text-xs text-fg-muted">{file.path}</code>
        {#if file.writable}
          <Badge tone="success">writable</Badge>
        {:else}
          <Badge tone="warning">{file.readOnlyReason ?? "read-only"}</Badge>
        {/if}
        {#if file.revision}
          <code class="text-xs text-fg-subtle" title={file.revision}
            >{file.revision.slice(0, 15)}</code>
        {:else}
          <span class="text-xs text-fg-subtle">not written yet</span>
        {/if}
      </li>
    {/each}
  </ul>
</Card>

{#if overrides.length}
  <Card title="Runtime overrides" description="Applied from here or the API, on top of the files">
    <ul class="-mx-5 divide-y border-y">
      {#each overrides as [key, override] (key)}
        <li class="flex flex-wrap items-center gap-inline px-5 py-2 text-sm">
          <code class="font-medium">{key}</code>
          <code class="text-fg-muted">{JSON.stringify(override.value)}</code>
          <span class="text-xs text-fg-subtle">
            by {override.setBy}, {override.untilRestart
              ? UNTIL_RESTART
              : override.expiresAt
                ? `until ${dateTime(override.expiresAt)}`
                : "until removed"}
          </span>
          <Button
            variant="ghost"
            size="icon-sm"
            class="ml-auto"
            aria-label={`Remove the override of ${key}`}
            disabled={busy.running}
            onclick={() => remove(key)}>
            <Trash2Icon />
          </Button>
        </li>
      {/each}
    </ul>
  </Card>
{/if}

<ConfirmDialog
  bind:open={restarting}
  title="Restart reseek"
  body="Transfers in progress stop and resume after the restart. The page comes back once reseek is up."
  confirm="Restart"
  danger
  onconfirm={restart} />
