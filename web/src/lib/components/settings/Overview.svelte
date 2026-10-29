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
  import { RESTART, UNTIL_RESTART } from "#lib/wording.js";

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
    }, `${key} restored to its configured value`);
</script>

{#if !remote}
  <Notice kind="warning">
    Remote configuration is off. Settings are read-only here. Set
    <code>remote_configuration</code> to change them.
  </Notice>
{/if}
{#if view.loadError}
  <Notice kind="error"
    >Could not reload the configuration. The previous values remain in effect: {view.loadError}</Notice>
{/if}
{#each view.warnings as warning (warning)}
  <Notice kind="warning">{warning}</Notice>
{/each}
{#if view.pendingRestart.length || app.state?.pendingRestart}
  <Notice kind="warning">
    Restart reseek to apply {view.pendingRestart.join(", ") || "a change"}.
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
    Reconnect to the Soulseek server to apply {view.pendingReconnect.join(", ") || "a change"}.
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
        <code class="min-w-0 text-xs wrap-anywhere text-fg-muted">{file.path}</code>
        {#if file.writable}
          <Badge tone="success">writable</Badge>
        {:else}
          <Badge tone="warning">{file.readOnlyReason ?? "read-only"}</Badge>
        {/if}
        {#if file.revision}
          <code class="text-xs text-fg-subtle" title={file.revision}
            >{file.revision.slice(0, 15)}</code>
        {:else}
          <span class="text-xs text-fg-subtle">Not written yet</span>
        {/if}
      </li>
    {/each}
  </ul>
</Card>

{#if overrides.length}
  <Card
    title="Runtime overrides"
    description="Values applied from this page or the API, on top of the configuration files">
    <ul class="-mx-5 divide-y border-y">
      {#each overrides as [key, override] (key)}
        <li class="flex flex-wrap items-center gap-inline px-5 py-2 text-sm">
          <code class="min-w-0 font-medium wrap-anywhere">{key}</code>
          <code class="min-w-0 wrap-anywhere text-fg-muted">{JSON.stringify(override.value)}</code>
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
  title={RESTART.title}
  body={RESTART.body}
  confirm={RESTART.confirm}
  danger
  onconfirm={restart} />
