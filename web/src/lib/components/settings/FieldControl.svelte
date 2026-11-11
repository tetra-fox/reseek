<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import { NOTIFY_MODES, type NotifyMode } from "#lib/chat/modes.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import AudiencePicker from "#lib/components/users/groups/AudiencePicker.svelte";
  import type { Draft } from "#lib/config/fields.js";
  import type { Control, Field } from "#lib/config/sections.js";
  import { REDACTED } from "#lib/config/tables.js";
  import { DEFAULT } from "#lib/users/groups.js";

  import TableEditor from "./TableEditor.svelte";

  type Props = {
    field: Field;
    control: Control;
    /** the names a select offers, or the groups a group list picks from */
    choices: string[];
    /** the fields of a table's rows */
    entries: Schemas["EntryView"][];
    draft: Draft;
    disabled: boolean;
    /** an id the help and error lines carry, for aria-describedby */
    described: string;
    /** what is wrong with the value, which a table shows by its rows */
    error: string | undefined;
  };

  let {
    field,
    control,
    choices,
    entries,
    draft = $bindable(),
    disabled,
    described,
    error
  }: Props = $props();

  const invalid = $derived(!!error);

  // a secret's input shows only once the user chooses to change it; `kept` is what it held then
  let changing = $state(false);
  let kept = $state<Draft>(REDACTED);
  const cleared = $derived(kept === REDACTED && draft === "");
  /** a group list's groups, one a line, with `default` standing for everyone */
  const groups = $derived.by(() => {
    const lines = String(draft)
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
    return lines.includes(DEFAULT) ? null : lines;
  });
  /** what `auto` stands for, shown while the input holds it */
  const autoHint = $derived(
    String(draft).trim().toLowerCase() === "auto" ? field.auto?.hint() : undefined
  );
</script>

{#if control === "bool"}
  <Switch
    id={field.key}
    class="mt-1.5"
    checked={draft === true}
    onCheckedChange={(v) => (draft = v)}
    aria-describedby={described}
    {disabled} />
{:else if control === "select"}
  <Select
    id={field.key}
    class="w-full"
    bind:value={draft}
    options={choices.map((c) => ({ value: c as Draft, label: c }))}
    {disabled} />
{:else if control === "notify-mode"}
  <Segmented
    id={field.key}
    class="w-fit"
    size="sm"
    options={NOTIFY_MODES}
    bind:value={draft as NotifyMode}
    label={field.label}
    aria-describedby={described}
    {disabled} />
{:else if control === "secret"}
  {#if changing}
    <!-- the value only changes once something is typed; clearing it is a choice of its own -->
    <div class="flex items-center gap-inline">
      <Input
        id={field.key}
        type="password"
        autocomplete="new-password"
        placeholder={cleared ? "Cleared on save" : "New value"}
        aria-describedby={described}
        aria-invalid={invalid || undefined}
        value={draft === kept || cleared ? "" : String(draft)}
        oninput={(e) => (draft = e.currentTarget.value || kept)}
        {disabled} />
      {#if kept === REDACTED}
        <Button variant="outline" size="sm" {disabled} onclick={() => (draft = cleared ? kept : "")}
          >{cleared ? "Keep" : "Clear"}</Button>
      {/if}
    </div>
  {:else}
    <div class="flex items-center gap-inline">
      <span class="text-sm text-fg-muted">{draft === REDACTED ? "set" : "not set"}</span>
      <Button
        id={field.key}
        variant="outline"
        size="sm"
        {disabled}
        onclick={() => {
          kept = draft;
          changing = true;
        }}>Change</Button>
    </div>
  {/if}
{:else if control === "groups"}
  <AudiencePicker
    id={field.key}
    value={groups}
    {choices}
    label={field.label}
    {disabled}
    class="w-fit min-w-40"
    onchange={(picked) => (draft = picked === null ? DEFAULT : picked.join("\n"))} />
{:else if control === "map" || control === "table-list"}
  <TableEditor {field} {control} {entries} bind:draft {disabled} {described} {error} />
{:else if control === "list" || control === "regex-list" || control === "cidr-list"}
  <!-- grows with its lines between the two heights, then scrolls; lists take one entry a line -->
  <Textarea
    id={field.key}
    class="field-sizing-content max-h-64 min-h-20 font-mono text-xs leading-5"
    spellcheck="false"
    placeholder="One per line"
    aria-describedby={described}
    aria-invalid={invalid || undefined}
    bind:value={draft as string}
    {disabled} />
{:else if control === "auto-count"}
  <!-- the hint goes under the input where the row has no room for both -->
  <div class="flex flex-wrap items-center gap-inline">
    <Input
      id={field.key}
      class="flex-1 basis-28 tabular-nums"
      placeholder="auto or a number"
      aria-describedby={autoHint === undefined ? described : `${described} ${field.key}-auto`}
      aria-invalid={invalid || undefined}
      bind:value={draft}
      {disabled} />
    {#if autoHint !== undefined}
      <span id={`${field.key}-auto`} class="text-sm text-fg-muted tabular-nums">{autoHint}</span>
    {/if}
  </div>
{:else}
  <Input
    id={field.key}
    class={control === "number" ? "tabular-nums" : "font-mono"}
    inputmode={control === "number" ? "decimal" : undefined}
    placeholder={field.placeholder ??
      (control === "duration"
        ? "For example 15m or 2h"
        : control === "size"
          ? "For example 10 MiB"
          : undefined)}
    aria-describedby={described}
    aria-invalid={invalid || undefined}
    bind:value={draft}
    {disabled} />
{/if}
