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
    draft: Draft;
    disabled: boolean;
    /** an id the help and error lines carry, for aria-describedby */
    described: string;
    invalid: boolean;
  };

  let { field, control, draft = $bindable(), disabled, described, invalid }: Props = $props();

  // a secret's input shows only once the user chooses to change it
  let changing = $state(false);
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
    options={(field.choices ?? []).map((c) => ({ value: c as Draft, label: c }))}
    {disabled} />
{:else if control === "secret"}
  {#if changing}
    <Input
      id={field.key}
      type="password"
      autocomplete="new-password"
      placeholder="empty clears it"
      aria-describedby={described}
      aria-invalid={invalid || undefined}
      bind:value={draft}
      {disabled} />
  {:else}
    <div class="flex items-center gap-inline">
      <span class="text-sm text-fg-muted">{draft === REDACTED ? "set" : "not set"}</span>
      <Button
        id={field.key}
        variant="outline"
        size="sm"
        {disabled}
        onclick={() => {
          changing = true;
          draft = "";
        }}>Change</Button>
    </div>
  {/if}
{:else if control === "list" || control === "regex-list" || control === "cidr-list" || control === "map"}
  <!-- grows with its lines between the two heights, then scrolls; lists take one entry a line -->
  <Textarea
    id={field.key}
    class="field-sizing-content max-h-64 min-h-20 font-mono text-xs leading-5"
    spellcheck="false"
    placeholder={control === "map" ? "{}" : "one per line"}
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
      (control === "duration" ? "like 15m or 2h" : control === "size" ? "like 10 MiB" : undefined)}
    aria-describedby={described}
    aria-invalid={invalid || undefined}
    bind:value={draft}
    {disabled} />
{/if}
