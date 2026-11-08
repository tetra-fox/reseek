<script lang="ts">
  import { beforeNavigate } from "$app/navigation";
  import BugIcon from "@lucide/svelte/icons/bug";
  import LockIcon from "@lucide/svelte/icons/lock";
  import { untrack } from "svelte";

  import { api, type Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { valueAt } from "#lib/config/sections.js";
  import { confirmation } from "#lib/feedback/confirm.svelte.js";
  import { plural } from "#lib/format.js";
  import { call } from "#lib/request.js";

  import CodeEditor from "./CodeEditor.svelte";
  import ErrorText from "./ErrorText.svelte";

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
  /**
   * the last save: running, or what it came to, `idle` when there is nothing to say; a refusal is
   * `stale` when the file changed on disk
   */
  type LastSave =
    | { kind: "idle" }
    | { kind: "running" }
    | { kind: "written"; written: Schemas["FileWritten"] }
    | { kind: "failed"; error: string; stale: boolean };
  let lastSave = $state.raw<LastSave>({ kind: "idle" });
  const running = $derived(lastSave.kind === "running");
  let debugText = $state<string | null>(null);
  /** why the debug view could not be read */
  let debugError = $state("");

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
    lastSave = { kind: "idle" };
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
    // the text back as the file has it: nothing to check, and an old answer goes with it
    if (!untrack(() => dirty)) {
      checks += 1;
      validating = false;
      validation = null;
      return;
    }
    const timer = setTimeout(() => void validate(value), VALIDATE_MS);
    return () => clearTimeout(timer);
  });

  /** Saves the text as it is now; what is typed while it runs stays a change to save */
  const save = async () => {
    const sent = text;
    const at = name;
    const revision = file?.revision ?? null;
    lastSave = { kind: "running" };
    const checked = await validate(sent);
    // another file was opened meanwhile, which starts its own state
    if (at !== name) return;
    if (!checked?.valid) {
      lastSave = { kind: "idle" };
      return;
    }
    const outcome = await call(() =>
      api.PUT("/api/v0/options/file", { body: { text: sent, revision, file: at } })
    );
    if (at !== name) return;
    if (!outcome.ok) {
      lastSave = { kind: "failed", error: outcome.error, stale: outcome.status === 409 };
      return;
    }
    lastSave = { kind: "written", written: outcome.data };
    file = file && { ...file, text: sent, revision: outcome.data.revision };
  };

  const discard = () =>
    confirmation.ask({
      title: "Discard changes?",
      body: "Your edits to the file will be lost",
      confirm: "Discard",
      run: () => void load()
    });

  beforeNavigate(({ cancel, type }) => {
    if (dirty && type !== "leave" && !confirm("Discard the changes to the configuration file?")) {
      cancel();
    }
  });
  // closing or reloading the tab asks too
  $effect(() => {
    if (!dirty) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  });

  const openDebug = async () => {
    const outcome = await call(() => api.GET("/api/v0/options/debug"));
    debugError = outcome.ok ? "" : outcome.error;
    if (outcome.ok) debugText = outcome.data;
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

{#if debugError}<Notice kind="error">Could not read the debug view: {debugError}</Notice>{/if}

{#if !remote}
  <Notice kind="info">
    <LockIcon class="inline size-3.5" /> Remote configuration is off. Set
    <code>remote_configuration</code> to edit the file here.
  </Notice>
{:else if !writable}
  <Notice kind="warning"
    >{info?.name} is not writable{info?.readOnlyReason ? ` (${info.readOnlyReason})` : ""}</Notice>
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
        ><Spinner class="size-3.5" />Checking...</span>
    {:else if validation?.valid && dirty}
      <span class="text-success">Valid</span>
    {/if}
    {#if writable}
      <div class="ml-auto flex gap-inline">
        <Button variant="ghost" disabled={!dirty || running} onclick={discard}>Discard</Button>
        <Button
          disabled={!dirty || running || validating || validation?.valid === false}
          onclick={() => void save()}>Save</Button>
      </div>
    {/if}
  </div>
  {#if validation && !validation.valid}
    <Notice kind="error">
      <ul class="flex flex-col gap-0.5">
        {#each validation.errors as e, i (`${i}:${e.key}`)}
          <li>
            {#if e.key}<code class="wrap-anywhere">{e.key}</code>:{" "}{/if}<ErrorText
              text={e.message} />
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
  {#if lastSave.kind === "failed"}
    {@const failed = lastSave}
    <Notice kind="error">
      {failed.stale
        ? `${name} changed on disk. Reload it, then make the changes again.`
        : failed.error}
      {#snippet action()}
        {#if failed.stale}<Button variant="outline" size="sm" onclick={() => void load()}
            >Reload</Button
          >{/if}
      {/snippet}
    </Notice>
  {/if}
  {#if lastSave.kind === "written"}
    {@const s = lastSave.written}
    <Notice kind="info">
      Saved.
      {#if s.applied.length}{plural(s.applied.length, "setting", "settings")} applied live.{/if}
      {#if s.reconnectRequired.length}
        <Badge tone="warning">reconnect</Badge> Reconnect to the Soulseek server to apply: {s.reconnectRequired.join(
          ", "
        )}.
      {/if}
      {#if s.restartRequired.length}
        <Badge tone="warning">restart</Badge> Restart reseek to apply: {s.restartRequired.join(
          ", "
        )}.
      {/if}
      {#snippet action()}
        <Button variant="ghost" size="sm" onclick={() => (lastSave = { kind: "idle" })}
          >Dismiss</Button>
      {/snippet}
    </Notice>
  {/if}
{/if}

<Dialog
  bind:open={() => debugText !== null, (o) => !o && (debugText = null)}
  title="Effective configuration and where each value comes from"
  class="w-[min(56rem,calc(100vw-2rem))]">
  <CodeEditor class="h-[60svh]" aria-label="Debug view" value={debugText ?? ""} readonly />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (debugText = null)}>Close</Button>
  {/snippet}
</Dialog>
