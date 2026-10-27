<script lang="ts">
  import type { Snippet } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import type { Editability } from "#lib/config/editability.js";
  import type { Draft } from "#lib/config/fields.js";
  import type { Control, Field } from "#lib/config/sections.js";

  import FieldControl from "./FieldControl.svelte";
  import SourceBadge from "./SourceBadge.svelte";

  type Props = {
    field: Field;
    info: Schemas["FieldView"];
    control: Control;
    edit: Editability;
    /** the running values, for a list entry that follows from another option */
    values: unknown;
    draft: Draft;
    changed: boolean;
    /** the running value changed while this field was being edited */
    moved: boolean;
    /** the option it depends on is set so that this one has no effect */
    unused: boolean;
    error: string | undefined;
    /** shown under the control, such as a check that goes with the option */
    extra?: Snippet;
    /** counts the saves of this option; the control starts over from the saved value on each */
    generation: number;
  };

  let {
    field,
    info,
    control,
    edit,
    values,
    draft = $bindable(),
    changed,
    moved,
    unused,
    error,
    extra,
    generation
  }: Props = $props();

  const help = $derived(`${field.key}-help`);
  const entry = $derived(field.derived?.entry(values) ?? null);
  const notApplied = $derived(info.notApplied);
</script>

<!-- one setting: what it is on the left, its control on the right, stacked on phones -->
<div
  class={[
    "grid gap-x-8 gap-y-2 px-5 py-4 md:grid-cols-[minmax(0,1fr)_minmax(0,22rem)] md:items-start",
    unused && "opacity-60"
  ]}>
  <div class="flex min-w-0 flex-col gap-1 md:pt-1.5">
    <div class="flex flex-wrap items-center gap-inline">
      <label for={field.key} class="text-sm font-medium">{field.label}</label>
      {#if changed}
        <span class="size-1.5 rounded-full bg-accent" title="Changed"></span>
        <span class="sr-only">changed</span>
      {/if}
      <SourceBadge source={info.source} />
      {#if info.apply !== "live"}<Badge tone="warning">{info.apply}</Badge>{/if}
    </div>
    <p id={help} class="text-xs text-pretty text-fg-muted">
      {info.doc}
      {#if info.default !== null && info.default !== undefined && !(Array.isArray(info.default) && !info.default.length)}
        <span class="text-fg-subtle"
          >Default <code class="font-mono"
            >{typeof info.default === "string" ? info.default : JSON.stringify(info.default)}</code
          >.</span>
      {/if}
      {#if info.envVar || info.cliFlag}
        <span class="font-mono text-fg-subtle">
          {[info.envVar, info.cliFlag].filter(Boolean).join(" · ")}
        </span>
      {/if}
    </p>
  </div>
  <div class="flex min-w-0 flex-col gap-1">
    {#key generation}
      <FieldControl
        {field}
        {control}
        choices={info.choices ?? []}
        bind:draft
        disabled={edit.mode === "locked" || unused}
        described={help}
        invalid={!!error} />
    {/key}
    {#if entry}
      <p class="font-mono text-xs text-fg-subtle">{entry}</p>
      <p class="text-xs text-fg-subtle">{field.derived?.note}</p>
    {/if}
    {#if error}<p class="text-xs text-danger" role="alert">{error}</p>{/if}
    {#if edit.notice}<p class="text-xs text-fg-muted">{edit.notice}</p>{/if}
    {#if notApplied}
      <p class="text-xs text-danger">
        The file's value <code>{JSON.stringify(notApplied.value)}</code> isn't running: {notApplied.reason}
      </p>
    {/if}
    {@render extra?.()}
    {#if moved}<p class="text-xs text-warning-soft-fg">The running value changed</p>{/if}
  </div>
</div>
