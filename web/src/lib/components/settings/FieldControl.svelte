<script lang="ts">
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import { type Draft, REDACTED } from "#lib/config/fields.js";
  import type { Control, Field } from "#lib/config/sections.js";

  type Props = {
    field: Field;
    control: Control;
    /** the names a select offers */
    choices: string[];
    draft: Draft;
    disabled: boolean;
    /** an id the help and error lines carry, for aria-describedby */
    described: string;
    invalid: boolean;
  };

  let {
    field,
    control,
    choices,
    draft = $bindable(),
    disabled,
    described,
    invalid
  }: Props = $props();

  // a secret's input shows only once the user chooses to change it; `kept` is what it held then
  let changing = $state(false);
  let kept = $state<Draft>(REDACTED);
  const cleared = $derived(kept === REDACTED && draft === "");
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
{:else if control === "secret"}
  {#if changing}
    <!-- the value only changes once something is typed; clearing it is a choice of its own -->
    <div class="flex items-center gap-inline">
      <Input
        id={field.key}
        type="password"
        autocomplete="new-password"
        placeholder={cleared ? "Cleared when you save" : "New value"}
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
{:else if control === "list" || control === "regex-list" || control === "cidr-list" || control === "map"}
  <!-- grows with its lines between the two heights, then scrolls; lists take one entry a line -->
  <Textarea
    id={field.key}
    class="field-sizing-content max-h-64 min-h-20 font-mono text-xs leading-5"
    spellcheck="false"
    placeholder={control === "map" ? "{}" : "One per line"}
    aria-describedby={described}
    aria-invalid={invalid || undefined}
    bind:value={draft as string}
    {disabled} />
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
