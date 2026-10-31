<script lang="ts">
  import CopyIcon from "@lucide/svelte/icons/copy";

  import { copyText } from "#lib/feedback/clipboard.svelte.js";
  import type { Field } from "#lib/inspector/inspector.svelte.js";

  // a section's lines, those without a value left out; `copy` is for raw values, each on its own
  // line in monospace with a button that copies it
  type Props = { fields: Field[]; copy?: boolean };

  let { fields, copy = false }: Props = $props();

  const shown = $derived(fields.flatMap((f) => (f.value ? [{ ...f, value: f.value }] : [])));
</script>

{#if copy}
  <dl class="flex flex-col gap-2">
    {#each shown as field (field.label)}
      <div>
        <dt class="text-xs text-fg-muted">{field.label}</dt>
        <dd class="flex items-start gap-1">
          <span
            class="min-w-0 flex-1 cursor-text font-mono text-xs wrap-anywhere whitespace-pre-wrap"
            >{field.value}</span>
          <button
            type="button"
            aria-label={`Copy ${field.label.toLowerCase()}`}
            title={`Copy ${field.label.toLowerCase()}`}
            onclick={() => void copyText(field.value)}>
            <CopyIcon class="size-3.5 text-fg-muted" />
          </button>
        </dd>
      </div>
    {/each}
  </dl>
{:else}
  <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1">
    {#each shown as field (field.label)}
      <dt class="text-fg-muted">{field.label}</dt>
      <dd class="wrap-anywhere">{field.value}</dd>
    {/each}
  </dl>
{/if}
