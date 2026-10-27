<script lang="ts">
  import { beforeNavigate } from "$app/navigation";
  import BugIcon from "@lucide/svelte/icons/bug";
  import LockIcon from "@lucide/svelte/icons/lock";
  import { untrack } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import CodeEditor from "#lib/components/CodeEditor.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { valueAt } from "#lib/config/sections.js";
  import { confirmation } from "#lib/confirm.svelte.js";
  import { plural } from "#lib/format.js";
  import { call } from "#lib/request.js";

  // a configuration file edited as text: checked as it is typed, saved against the revision read
  let { view }: { view: Schemas["ConfigView"] } = $props();

  /** how long typing rests before the text is checked */
  const VALIDATE_MS = 500;

  // svelte-ignore state_referenced_locally
  let name = $state(view.files[0]?.name ?? "config.toml");
  let file = $state.raw<Schemas["ConfigFile"] | null>(null);
  let text = $state("");
  let loadError = $state("");
  let validation = $state.raw<Schemas["Validation"] | null>(null);
  let validating = $state(false);
  let saving = $state(false);
  let saveError = $state("");
  let stale = $state(false);
  let saved = $state.raw<Schemas["FileWritten"] | null>(null);
  let debugText = $state<string | null>(null);

  const remote = $derived(valueAt(view.values, "remote_configuration") === true);
  const debug = $derived(valueAt(view.values, "debug") === true);
  const info = $derived(view.files.find((f) => f.name === name));
  const writable = $derived(remote && !!info?.writable && !view.managedBy);
  const dirty = $derived(!!file && text !== (file.text ?? ""));

  const load = async () => {
    const outcome = await call(() =>
      api.GET("/api/v0/options/file", { params: { query: { file: name } } })
    );
    if (!outcome.ok) return void (loadError = outcome.error);
    loadError = "";
    file = outcome.data;
    text = outcome.data.text ?? "";
    validation = null;
    stale = false;
  };
  $effect(() => {
    void name;
    untrack(() => void load());
  });

  // the newest check wins; an answer to an older text is dropped
  let checks = 0;
  const validate = async (value: string) => {
    const at = ++checks;
    validating = true;
    const outcome = await call(() =>
      api.POST("/api/v0/options/file/validate", { body: { text: value, file: name } })
    );
    if (at !== checks) return null;
    validating = false;
    const result = outcome.ok
      ? outcome.data
      : { valid: false, errors: [{ key: null, message: outcome.error }], warnings: [] };
    validation = result;
    return result;
  };
  $effect(() => {
    const value = text;
    if (!untrack(() => dirty)) return;
    const timer = setTimeout(() => void validate(value), VALIDATE_MS);
    return () => clearTimeout(timer);
  });

  const save = async () => {
    saving = true;
    saveError = "";
    const checked = await validate(text);
    if (!checked?.valid) {
      saving = false;
      return;
    }
    const outcome = await call(() =>
      api.PUT("/api/v0/options/file", {
        body: { text, revision: file?.revision ?? null, file: name }
      })
    );
    saving = false;
    if (!outcome.ok) {
      if (outcome.status === 409) stale = true;
      saveError = outcome.error;
      return;
    }
    saved = outcome.data;
    file = file && { ...file, text, revision: outcome.data.revision };
  };

  const discard = () =>
    confirmation.ask({
      title: "Discard changes?",
      body: "The edits made here go; the file stays as it is.",
      confirm: "Discard",
      run: () => void load()
    });

  beforeNavigate(({ cancel, type }) => {
    if (dirty && type !== "leave" && !confirm("Discard the changes to the configuration file?")) {
      cancel();
    }
  });

  const openDebug = async () => {
    const outcome = await call(() => api.GET("/api/v0/options/debug"));
    if (outcome.ok) debugText = outcome.data;
    else saveError = outcome.error;
  };
</script>

<div class="flex flex-wrap items-center gap-inline">
  {#if view.files.length > 1}
    <Select
      class="w-64"
      aria-label="File"
      disabled={dirty}
      bind:value={name}
      options={view.files.map((f) => ({ value: f.name, label: f.name }))} />
  {/if}
  {#if file}<span class="font-mono text-xs text-fg-muted">{file.path}</span>{/if}
  <div class="ml-auto flex gap-inline">
    {#if debug && remote}
      <Button variant="outline" size="sm" onclick={() => void openDebug()}
        ><BugIcon />Debug view</Button>
    {/if}
  </div>
</div>

{#if !remote}
  <Notice kind="info">
    <LockIcon class="inline size-3.5" /> Remote configuration is off: set
    <code>remote_configuration</code> to edit the file from here.
  </Notice>
{:else if !writable}
  <Notice kind="warning"
    >{info?.name} isn't writable{info?.readOnlyReason ? ` (${info.readOnlyReason})` : ""}.</Notice>
{:else if file}
  <Notice kind="warning">Editing {file.path}</Notice>
{/if}

{#if loadError}
  <Notice kind="error">{loadError}</Notice>
{:else if !file}
  <Skeleton class="h-96" />
{:else}
  <CodeEditor
    class="h-[60svh] min-h-96"
    aria-label={`Contents of ${name}`}
    value={text}
    readonly={!writable}
    onchange={(t) => (text = t)} />
  <div class="flex flex-wrap items-center gap-inline text-sm">
    {#if validating}
      <span class="flex items-center gap-1 text-fg-muted"
        ><Spinner class="size-3.5" />checking…</span>
    {:else if validation?.valid && dirty}
      <span class="text-success">Valid</span>
    {/if}
    {#if writable}
      <div class="ml-auto flex gap-inline">
        <Button variant="ghost" disabled={!dirty || saving} onclick={discard}>Discard</Button>
        <Button
          disabled={!dirty || saving || validating || validation?.valid === false}
          onclick={() => void save()}>Save</Button>
      </div>
    {/if}
  </div>
  {#if validation && !validation.valid}
    <Notice kind="error">
      <ul class="flex flex-col gap-0.5">
        {#each validation.errors as e, i (`${i}:${e.key}`)}
          <li>
            {#if e.key}<code>{e.key}</code>:
            {/if}{e.message}
          </li>
        {/each}
      </ul>
    </Notice>
  {/if}
  {#if validation?.warnings.length}
    <Notice kind="warning">
      <ul class="flex flex-col gap-0.5">
        {#each validation.warnings as w (w)}<li>{w}</li>{/each}
      </ul>
    </Notice>
  {/if}
  {#if saveError}
    <Notice kind="error">
      {stale ? `${name} changed on disk; reload it, then make the change again.` : saveError}
      {#snippet action()}
        {#if stale}<Button variant="outline" size="sm" onclick={() => void load()}>Reload</Button
          >{/if}
      {/snippet}
    </Notice>
  {/if}
  {#if saved}
    {@const s = saved}
    <Notice kind="info">
      Saved.
      {#if s.applied.length}{plural(s.applied.length, "setting", "settings")} applied live.{/if}
      {#if s.reconnectRequired.length}
        <Badge tone="warning">reconnect</Badge> Reconnect to the Soulseek server to apply: {s.reconnectRequired.join(
          ", "
        )}.
      {/if}
      {#if s.restartRequired.length}
        <Badge tone="warning">restart</Badge> Restart to apply: {s.restartRequired.join(", ")}.
      {/if}
      {#snippet action()}
        <Button variant="ghost" size="sm" onclick={() => (saved = null)}>Dismiss</Button>
      {/snippet}
    </Notice>
  {/if}
{/if}

<Dialog
  bind:open={() => debugText !== null, (o) => !o && (debugText = null)}
  title="Effective configuration with source per key"
  class="w-[min(56rem,calc(100vw-2rem))]">
  <CodeEditor class="h-[60svh]" aria-label="Debug view" value={debugText ?? ""} readonly />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (debugText = null)}>Close</Button>
  {/snippet}
</Dialog>
