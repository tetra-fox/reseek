<script lang="ts">
  import CopyIcon from "@lucide/svelte/icons/copy";
  import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
  import TerminalIcon from "@lucide/svelte/icons/terminal";
  import { LinkPreview } from "bits-ui";
  import type { Snippet } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
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
    /** the value running is not the default; null when that can't be judged, as for a secret */
    custom: boolean | null;
    /** puts the default back in the draft, which a save then writes by taking the key out */
    ondefault: () => void;
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
    ondefault
  }: Props = $props();

  const help = $derived(`${field.key}-help`);
  /** a value as the help writes it */
  const shown = (value: unknown) => (typeof value === "string" ? value : JSON.stringify(value));
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
        <span class="size-1.5 rounded-full bg-accent" title="Not saved yet"></span>
        <span class="sr-only">not saved yet</span>
      {/if}
      {#if custom && edit.mode !== "locked"}
        <button
          type="button"
          class="flex items-center gap-1 rounded-badge border border-dashed border-accent/50 px-1.5 text-xs text-accent-soft-fg hover:bg-accent-soft"
          title={`Changed from the default, ${shown(info.default)}. Click to put the default back`}
          onclick={ondefault}>
          <RotateCcwIcon class="size-3" />default
        </button>
      {:else if custom}
        <Badge tone="outline" title={`The default is ${shown(info.default)}`}>not default</Badge>
      {/if}
      <SourceBadge source={info.source} />
      {#if info.apply !== "live"}<Badge tone="warning">{info.apply}</Badge>{/if}
    </div>
    <p id={help} class="text-xs text-pretty text-fg-muted">
      {info.doc}
      {#if info.default !== null && info.default !== undefined && !(Array.isArray(info.default) && !info.default.length)}
        <span class="text-fg-subtle"
          >Default <code class="font-mono">{shown(info.default)}</code>.</span>
      {/if}
    </p>
    {#if info.envVar || info.cliFlag}
      <LinkPreview.Root openDelay={250} closeDelay={150}>
        <LinkPreview.Trigger>
          {#snippet child({ props })}
            <button
              {...props}
              type="button"
              class="flex w-fit items-center gap-1 rounded-badge text-xs text-fg-subtle hover:text-fg"
              aria-label={`Set ${field.label} from the environment or the command line`}>
              <TerminalIcon class="size-3.5" />Environment and flag
            </button>
          {/snippet}
        </LinkPreview.Trigger>
        <LinkPreview.Portal>
          <LinkPreview.Content
            side="bottom"
            align="start"
            sideOffset={4}
            class="z-50 flex w-max max-w-[calc(100vw-1rem)] flex-col gap-1.5 rounded-card border bg-overlay p-3 text-xs shadow-raised motion-safe:animate-popover-in">
            <p class="text-fg-muted">Set it outside the file, where scripts and services do:</p>
            {#each [info.envVar && `${info.envVar}=…`, info.cliFlag && `${info.cliFlag} …`].filter((t): t is string => !!t) as text (text)}
              <span class="flex items-center gap-2">
                <code class="flex-1 rounded-badge bg-surface-muted px-1.5 py-0.5 font-mono"
                  >{text}</code>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Copy ${text.split(/[= ]/)[0]}`}
                  onclick={() => void copyText(text.split(/[= ]/)[0])}><CopyIcon /></Button>
              </span>
            {/each}
          </LinkPreview.Content>
        </LinkPreview.Portal>
      </LinkPreview.Root>
    {/if}
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
