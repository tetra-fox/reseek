<script lang="ts">
  import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
  import type { Snippet } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import type { Editability } from "#lib/config/editability.js";
  import type { Draft } from "#lib/config/fields.js";
  import { helpParts } from "#lib/config/help.js";
  import type { Control, Field } from "#lib/config/sections.js";

  import EnvAndFlag from "./EnvAndFlag.svelte";
  import ErrorText from "./ErrorText.svelte";
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
    /** the value running is not the default; null when that can't be judged, as for a secret */
    custom: boolean | null;
    /** puts the default back in the draft, which a save then writes by taking the key out */
    ondefault: () => void;
    /** why the value can't be cleared from here, null when it can */
    held: string | null;
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
    generation,
    custom,
    ondefault,
    held
  }: Props = $props();

  const help = $derived(`${field.key}-help`);
  /** a value as the help writes it */
  const shown = (value: unknown) => (typeof value === "string" ? value : JSON.stringify(value));
  const entry = $derived(field.derived?.entry(values) ?? null);
  const notApplied = $derived(info.notApplied);
  /** a table takes the row's whole width, and says what is wrong by its rows */
  const tabular = $derived(control === "map" || control === "table-list");
  /** whether the control is one element a label can name */
  const labelable = $derived(!tabular && control !== "notify-mode");
  const restorable = $derived(!!custom && edit.mode !== "locked");
  const environment = $derived(!!(info.envVar || info.cliFlag));
  /** the row has buttons that go with the option rather than its value */
  const tools = $derived(restorable || environment);
</script>

<!-- one setting: what it is on the left, its control on the right, stacked on phones, and a table
     under what it is at any width. The restore and environment buttons come after the control:
     under the help on a wider screen, under the control on a phone and under a table -->
<div
  class={[
    "grid grid-cols-[minmax(0,1fr)] gap-x-8 gap-y-2 px-5 py-4 md:items-start",
    !tabular && "md:grid-cols-[minmax(0,1fr)_minmax(0,22rem)]",
    unused && "opacity-60"
  ]}>
  <div class="flex min-w-0 flex-col gap-1 md:pt-1.5">
    <div class="flex flex-wrap items-center gap-inline">
      <!-- a group of radios or a table is no element a label can name; it carries the name itself -->
      <label
        for={labelable ? field.key : undefined}
        class="min-w-0 text-sm font-medium wrap-anywhere">{field.label}</label>
      {#if changed}
        <Tooltip text="Not saved yet" dense>
          {#snippet child({ props })}
            <span {...props} class="size-1.5 rounded-full bg-accent"></span>
          {/snippet}
        </Tooltip>
        <span class="sr-only">not saved yet</span>
      {/if}
      {#if custom && edit.mode === "locked"}
        <Tooltip text={tabular ? undefined : `The default is ${shown(info.default)}`} dense spoken>
          {#snippet child({ props })}<Badge {...props} tone="outline">not default</Badge>{/snippet}
        </Tooltip>
      {/if}
      <SourceBadge source={info.source} />
      {#if info.apply !== "live"}<Badge tone="warning">Applies on {info.apply}</Badge>{/if}
    </div>
    <p id={help} class="text-xs text-pretty wrap-anywhere text-fg-muted">
      {#each helpParts(info.doc) as part, i (i)}{#if part.code}<code class="font-mono wrap-anywhere"
            >{part.text}</code
          >{:else}{part.text}{/if}{/each}
      {#if !tabular && info.default !== null && info.default !== undefined && !(Array.isArray(info.default) && !info.default.length)}
        <span class="text-fg-subtle"
          >Default <code class="font-mono wrap-anywhere">{shown(info.default)}</code></span>
      {/if}
    </p>
  </div>
  <div
    class={[
      "flex min-w-0 flex-col gap-1",
      !tabular && "md:col-start-2 md:row-start-1",
      !tabular && tools && "md:row-span-2"
    ]}>
    {#key generation}
      <FieldControl
        {field}
        {control}
        choices={info.choices ?? []}
        entries={info.entries}
        bind:draft
        disabled={edit.mode === "locked" || unused}
        described={help}
        {error} />
    {/key}
    {#if entry}
      <p class="font-mono text-xs wrap-anywhere text-fg-subtle">{entry}</p>
      <p class="text-xs text-fg-subtle">{field.derived?.note}</p>
    {/if}
    {#if error && !tabular}<p class="text-xs text-danger" role="alert">
        <ErrorText text={error} />
      </p>{/if}
    {#if edit.notice}<p class="text-xs text-fg-muted">{edit.notice}</p>{/if}
    {#if notApplied}
      <p class="text-xs text-danger">
        The value <code class="wrap-anywhere">{JSON.stringify(notApplied.value)}</code> in the file
        is not in effect: <ErrorText text={notApplied.reason} />
      </p>
    {/if}
    {@render extra?.()}
    {#if moved}<p class="text-xs text-warning-soft-fg">The running value has changed</p>{/if}
  </div>
  {#if tools}
    <div
      class={[
        "flex flex-wrap items-center gap-x-3 gap-y-1",
        !tabular && "md:col-start-1 md:row-start-2"
      ]}>
      {#if restorable}
        <Tooltip
          text={held ?? (tabular ? "Restore default" : `Restore default: ${shown(info.default)}`)}>
          {#snippet child({ props })}
            <!-- a dashed badge rather than a control's box, so it reads as a mark on the value -->
            <Button
              {...props}
              variant="link"
              class="gap-1 rounded-badge border border-dashed border-accent/50 px-1.5 text-xs font-normal text-accent-soft-fg usable:hover:bg-accent-soft usable:hover:no-underline"
              aria-label={`Restore default of ${field.label}`}
              aria-disabled={held ? true : undefined}
              onclick={ondefault}>
              <RotateCcwIcon class="size-3" />default
            </Button>
          {/snippet}
        </Tooltip>
      {/if}
      {#if environment}
        <EnvAndFlag label={field.label} envVar={info.envVar} cliFlag={info.cliFlag} />
      {/if}
    </div>
  {/if}
</div>
