<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import ClipboardListIcon from "@lucide/svelte/icons/clipboard-list";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import PowerIcon from "@lucide/svelte/icons/power";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import StarIcon from "@lucide/svelte/icons/star";
  import UnplugIcon from "@lucide/svelte/icons/unplug";

  import { api } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { Busy } from "#lib/feedback/busy.svelte.js";
  import { CopyButton, copyText } from "#lib/feedback/clipboard.svelte.js";
  import { confirmation } from "#lib/feedback/confirm.svelte.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { ago, formatBytes, formatDuration, formatSpeed, plural } from "#lib/format.js";
  import { serverStateText } from "#lib/reachability.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { diagnostics, latestProblems } from "#lib/system/diagnostics.js";
  import { clock } from "#lib/ui/clock.svelte.js";
  import { RESTART } from "#lib/wording.js";

  import Uptime from "./Uptime.svelte";

  // the daemon's state at a glance, its whole state document, and the process controls
  const s = $derived(app.state);
  const write = $derived(session.can("ReadWrite"));
  const control = $derived(session.can("Administrator") && session.canSignOut);
  const busy = new Busy();
  const admin = $derived(session.can("Administrator"));

  // the agent's socket closes and its shares go; it connects again unless its configuration went
  const disconnectAgent = (name: string) =>
    busy.run(() =>
      call(() => api.DELETE("/api/v0/relay/agents/{name}", { params: { path: { name } } }))
    );

  const checkVersion = () =>
    busy.run(async () => {
      const outcome = await call(() =>
        api.GET("/api/v0/application/version/latest", { params: { query: { forceCheck: true } } })
      );
      if (outcome.ok) {
        const v = outcome.data;
        toast(
          v.isUpdateAvailable && v.latest
            ? `Version ${v.latest} is available`
            : `Version ${v.current} is the latest`
        );
      }
      return outcome;
    });

  const reconnect = () =>
    confirmation.ask({
      title: "Reconnect?",
      body: "Transfers in progress are interrupted while reseek reconnects to the Soulseek server.",
      confirm: "Reconnect",
      run: () =>
        void busy.run(async () => {
          const off = await call(() =>
            api.DELETE("/api/v0/server", { body: "reconnecting from the web UI" })
          );
          return off.ok ? call(() => api.PUT("/api/v0/server")) : off;
        })
    });
  const restart = () =>
    confirmation.ask({
      ...RESTART,
      run: () => void busy.run(() => call(() => api.PUT("/api/v0/application")))
    });
  const shutDown = () =>
    confirmation.ask({
      title: "Shut down reseek?",
      body: "reseek must then be started again manually.",
      confirm: "Shut down",
      run: () => void busy.run(() => call(() => api.DELETE("/api/v0/application")))
    });

  const dump = $derived(s ? JSON.stringify(s, null, 2) : "");

  const copying = new CopyButton();
  /** Copies what a bug report needs; an administrator's also carries the latest log problems */
  const copyDiagnostics = async () => {
    if (!s) return;
    const log = session.can("Administrator")
      ? await call(() => api.GET("/api/v0/logs/live"))
      : null;
    const problems = log?.ok ? latestProblems(log.data) : [];
    await copying.copy(diagnostics(s, navigator.userAgent, problems));
  };
</script>

<div class="flex flex-wrap items-center gap-inline">
  {#if write}
    <Button variant="outline" disabled={busy.running || !s} onclick={() => void checkVersion()}
      ><RefreshCwIcon />Check for updates</Button>
  {/if}
  {#if s?.user.username}
    <Button
      variant="outline"
      href={`https://www.slsknet.org/qtlogin.php?username=${encodeURIComponent(s.user.username)}`}
      target="_blank"
      rel="noopener"><StarIcon class="text-warning" />Get privileges</Button>
  {/if}
  <Button variant="outline" disabled={!s} onclick={() => void copyDiagnostics()}>
    {#if copying.copied}<CheckIcon />Copied{:else}<ClipboardListIcon />Copy diagnostics{/if}
  </Button>
  <div class="ml-auto flex flex-wrap gap-inline">
    {#if write}
      <Button
        variant={s?.pendingReconnect ? "secondary" : "outline"}
        class={s?.pendingReconnect ? "text-warning" : undefined}
        disabled={busy.running || !s}
        onclick={reconnect}><UnplugIcon />Reconnect</Button>
    {/if}
    {#if control}
      <Button
        variant="outline"
        class={s?.pendingRestart ? "text-warning" : undefined}
        disabled={busy.running || !s}
        onclick={restart}><RotateCwIcon />Restart</Button>
      <Button variant="danger" disabled={busy.running || !s} onclick={shutDown}
        ><PowerIcon />Shut down</Button>
    {/if}
  </div>
</div>

{#if !s}
  <Skeleton class="h-64" />
{:else}
  <div class="grid grid-cols-[minmax(0,1fr)] gap-section md:grid-cols-2 xl:grid-cols-3">
    <Card title="Version">
      <dl class="info">
        <dt>Running</dt>
        <dd class="font-mono">{s.version.full}</dd>
        <dt>Latest</dt>
        <dd>
          {s.version.latest ?? "Not checked"}
          {#if s.version.isUpdateAvailable}<span class="text-warning">(update available)</span>{/if}
        </dd>
        {#if s.pendingRestart || s.pendingReconnect}
          <dt>Pending</dt>
          <dd class="text-warning">
            {[s.pendingRestart && "a restart", s.pendingReconnect && "a reconnect"]
              .filter(Boolean)
              .join(" and ")}
          </dd>
        {/if}
        {#if s.configuration.lastReloadAt}
          <dt>Configuration loaded</dt>
          <dd>{ago(s.configuration.lastReloadAt, clock.now)}</dd>
        {/if}
        {#if s.configuration.lastReloadError}
          <dt>Last reload error</dt>
          <dd class="text-danger">{s.configuration.lastReloadError}</dd>
        {/if}
      </dl>
    </Card>
    <Card title="Process">
      <dl class="info">
        <dt>Running for</dt>
        <dd><Uptime since={s.startedAt} /></dd>
        <dt>Process ID</dt>
        <dd>{s.process.pid}</dd>
        <dt>Platform</dt>
        <dd>{s.process.os} ({s.process.arch})</dd>
        <dt>Memory</dt>
        <dd>{s.process.memoryBytes ? formatBytes(s.process.memoryBytes) : "Unknown"}</dd>
        <dt>Web addresses</dt>
        <dd class="font-mono">
          {#each s.listeners as url (url)}<span class="block">{url}</span>{:else}None{/each}
        </dd>
      </dl>
    </Card>
    <Card title="Soulseek">
      <dl class="info">
        <dt>State</dt>
        <dd>{serverStateText(s.server)}</dd>
        <dt>Server</dt>
        <dd class="font-mono">
          {s.server.address ?? "?"}
          {s.server.ipEndPoint ? `(${s.server.ipEndPoint})` : ""}
        </dd>
        <dt>Account</dt>
        <dd>{s.user.username ?? "?"}</dd>
        <dt>Privileges</dt>
        <dd>
          {s.user.privileges.isPrivileged
            ? `until ${s.user.privileges.privilegedUntil ? new Date(s.user.privileges.privilegedUntil).toLocaleString() : formatDuration(s.user.privileges.privilegesRemaining)}`
            : "none"}
        </dd>
        <dt>Auto-reconnect</dt>
        <dd>
          {s.connectionWatchdog.isEnabled ? "on" : "off"}
          {#if s.connectionWatchdog.nextAttemptAt}, next {ago(
              s.connectionWatchdog.nextAttemptAt,
              clock.now
            )}{/if}
        </dd>
      </dl>
    </Card>
    <Card title="Sharing">
      <dl class="info">
        <dt>Shares</dt>
        <dd>
          {plural(s.shares.files, "file", "files")} in {plural(
            s.shares.directories,
            "folder",
            "folders"
          )}
        </dd>
        <dt>Seen by others</dt>
        <dd>
          {plural(s.user.statistics.fileCount, "file", "files")}, {formatSpeed(
            s.user.statistics.averageSpeed
          )},
          {s.user.statistics.uploadCount.toLocaleString()} uploads
        </dd>
        <dt>Incoming searches</dt>
        <dd>
          {s.health.search.incoming.queueDepth} queued, {Math.round(
            s.health.search.incoming.latency
          )} ms response time,
          {s.health.search.incoming.dropRate} dropped this minute
        </dd>
      </dl>
    </Card>
    <Card title="Distributed network">
      <dl class="info">
        <dt>Parent</dt>
        <dd>
          {s.distributedNetwork.parent ??
            (s.distributedNetwork.isBranchRoot ? "none (branch root)" : "none")}
        </dd>
        <dt>Branch</dt>
        <dd>
          level {s.distributedNetwork.branchLevel}, root {s.distributedNetwork.branchRoot ?? "?"}
        </dd>
        <dt>Children</dt>
        <dd>
          {s.distributedNetwork.children.length} of {s.distributedNetwork.childLimit}, {s
            .distributedNetwork.canAcceptChildren
            ? "accepting more"
            : "not accepting more"}
        </dd>
      </dl>
    </Card>
    <Card title="Relay">
      <dl class="info">
        <dt>Mode</dt>
        <dd>{s.relay.mode}</dd>
        {#if s.relay.mode !== "Controller"}
          <dt>Controller</dt>
          <dd>
            <span class="font-mono">{s.relay.controller.address ?? "?"}</span>,
            {s.relay.controller.state}
          </dd>
        {/if}
        {#if s.relay.mode !== "Agent"}
          <dt>Agents</dt>
          <dd>
            {#each s.relay.agents as agent (agent.name)}
              <span class="flex flex-wrap items-center gap-inline">
                <span title={`${agent.ipAddress}, version ${agent.version}`}>{agent.name}</span>
                <span class="text-fg-muted">
                  {agent.ipAddress}, since {ago(agent.connectedAt, clock.now)},
                  {agent.ready
                    ? `shares from ${agent.sharesUpdatedAt ? ago(agent.sharesUpdatedAt, clock.now) : "?"}`
                    : "sending shares"}
                </span>
                {#if admin}
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={busy.running}
                    onclick={() => void disconnectAgent(agent.name)}>Disconnect</Button>
                {/if}
              </span>
            {:else}none connected{/each}
          </dd>
        {/if}
      </dl>
    </Card>
  </div>

  <Card title="State">
    {#snippet action()}
      <Button variant="ghost" size="sm" onclick={() => void copyText(dump)}
        ><CopyIcon />Copy</Button>
    {/snippet}
    <pre
      class="max-h-[50svh] overflow-auto rounded-control bg-surface-muted p-3 font-mono text-xs">{dump}</pre>
  </Card>
{/if}

<style>
  /* a value too long for its card, an address or a version, wraps rather than being cut or
     widening the card */
  .info {
    display: grid;
    grid-template-columns: minmax(0, max-content) minmax(0, 1fr);
    gap: 0.25rem 1rem;
    font-size: 0.875rem;
  }
  .info :global(dd) {
    overflow-wrap: anywhere;
  }
  .info :global(dt) {
    color: var(--color-fg-muted);
  }
</style>
