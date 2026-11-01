<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { tick, untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Switch from "#lib/components/ui/Switch.svelte";
  import Textarea from "#lib/components/ui/Textarea.svelte";
  import type { Draft } from "#lib/config/fields.js";
  import { helpParts } from "#lib/config/help.js";
  import type { Control, Field } from "#lib/config/sections.js";
  import {
    cellOf,
    type Column,
    columnsOf,
    defaultText,
    draftOf,
    isHidden,
    type Row,
    rowErrors,
    rowProblems,
    rowsOf,
    typedNumber,
    withCell
  } from "#lib/config/tables.js";

  // an option that holds rows: a table of a line each while a row's fields fit one, past that each
  // row a section of its own that opens. Its fields come from the daemon's description of the
  // option, so a new field shows without a change here. The draft stays the option's whole value
  type Props = {
    field: Field;
    control: Control;
    /** the fields of a row, as the daemon describes them */
    entries: Schemas["EntryView"][];
    draft: Draft;
    disabled: boolean;
    /** an id the help line carries, for aria-describedby */
    described: string;
    /** what the daemon said about the option, which may name rows */
    error: string | undefined;
  };

  let {
    field,
    control,
    entries,
    draft = $bindable(),
    disabled,
    described,
    error
  }: Props = $props();

  /** the most fields a row may have and still fit a line */
  const MOST_ON_A_LINE = 4;

  const columns = $derived(columnsOf(field, entries));
  const onALine = $derived(columns.length <= MOST_ON_A_LINE);
  const named = $derived(control === "map");
  const nameLabel = $derived(field.rowName ?? "Name");

  /** the id the next row read or added takes, so a row keeps its element while others change */
  let next = 0;
  const read = () => {
    const held = rowsOf(control, String(draft), next);
    next += held.length;
    return held;
  };
  let rows = $state.raw<Row[]>(untrack(read));
  /** the draft as this control last wrote it; any other value came from outside */
  let written = untrack(() => draft);
  $effect(() => {
    if (draft === written) return;
    untrack(() => {
      written = draft;
      rows = read();
    });
  });

  /** rows added since the control started, which open by themselves */
  const added = new SvelteSet<number>();
  /** what each secret cell held before it was typed in, to go back to when it is emptied */
  const before = new Map<string, unknown>();
  let root = $state<HTMLElement | null>(null);

  const problems = $derived(rowProblems(control, rows, columns));
  const said = $derived(rowErrors(field.key, control, rows, error));
  /** the daemon's and a refused save's messages that name no row, which go over the table */
  const unplaced = $derived(said.rest.filter((m) => ![...problems.values()].includes(m)));
  /** A row just added says nothing about its empty name until a save has been refused */
  const fresh = (row: Row) => added.has(row.id) && !row.name.trim() && !error;
  const trouble = (row: Row) =>
    fresh(row) ? undefined : (problems.get(row.id) ?? said.byRow.get(row.id));

  const commit = (next: Row[]) => {
    rows = next;
    written = draftOf(control, rows);
    draft = written;
  };
  const change = (row: Row, changed: Row) =>
    commit(rows.map((r) => (r.id === row.id ? changed : r)));
  const setCell = (row: Row, key: string, cell: unknown) => change(row, withCell(row, key, cell));

  const typeSecret = (row: Row, key: string, text: string) => {
    const slot = `${row.id}:${key}`;
    if (!before.has(slot)) before.set(slot, cellOf(row, key));
    setCell(row, key, text || before.get(slot));
  };

  /** Focuses the first control of the row with `id`, or the Add button when there is no row */
  const focusRow = async (id: number | undefined) => {
    await tick();
    const place = id === undefined ? null : root?.querySelector(`[data-row="${id}"]`);
    const target = (place ?? root)?.querySelector<HTMLElement>(
      place ? "input, textarea, [role=combobox]" : "[data-add]"
    );
    target?.focus();
  };

  const add = () => {
    const id = next++;
    added.add(id);
    commit([...rows, { id, name: "", value: {} }]);
    void focusRow(id);
  };

  const remove = (row: Row) => {
    const at = rows.indexOf(row);
    commit(rows.filter((r) => r !== row));
    void focusRow((rows[at] ?? rows[at - 1])?.id);
  };

  /** How a row is named to assistive technology and on its section */
  const nameOf = (row: Row, i: number) => row.name.trim() || `row ${i + 1}`;
  const lines = (cell: unknown) => (Array.isArray(cell) ? cell.join("\n") : "");
  const fromLines = (text: string) =>
    text
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);

  /** A cell's control: plain text in its row, a tint under the pointer and a border in focus */
  const INLINE =
    "h-control-sm rounded-none border-transparent bg-transparent px-2 shadow-none hover:border-transparent hover:bg-hover focus-visible:bg-surface";
</script>

{#snippet nameCell(row: Row, i: number)}
  <Input
    class={[INLINE, "font-mono"].join(" ")}
    aria-label={`${nameLabel} of row ${i + 1}`}
    aria-invalid={trouble(row) ? true : undefined}
    value={row.name}
    oninput={(e) => change(row, { ...row, name: e.currentTarget.value })}
    {disabled} />
{/snippet}

{#snippet cell(row: Row, i: number, column: Column)}
  {@const held = cellOf(row, column.key)}
  {@const label = `${column.label} of ${nameOf(row, i)}`}
  {#if column.kind === "bool"}
    <Switch
      class="mx-2 mt-2"
      aria-label={label}
      checked={(held ?? column.default) === true}
      onCheckedChange={(v) => setCell(row, column.key, v)}
      {disabled} />
  {:else if column.kind === "select"}
    <Select
      class={INLINE}
      aria-label={label}
      bind:value={() => String(held ?? ""), (v) => setCell(row, column.key, v)}
      options={[
        { value: "", label: column.default ? `Default (${column.default})` : "Default" },
        ...column.choices.map((c) => ({ value: c, label: c }))
      ]}
      {disabled} />
  {:else if column.kind === "list"}
    <Textarea
      class={[
        INLINE,
        "field-sizing-content h-auto max-h-40 py-1.5 font-mono text-xs leading-5"
      ].join(" ")}
      spellcheck="false"
      aria-label={label}
      placeholder={defaultText(column) || "One per line"}
      value={lines(held)}
      oninput={(e) => setCell(row, column.key, fromLines(e.currentTarget.value))}
      {disabled} />
  {:else if column.kind === "secret"}
    <Input
      class={INLINE}
      type="password"
      autocomplete="new-password"
      aria-label={label}
      placeholder={isHidden(held) ? "Kept. Type to replace" : ""}
      value={isHidden(held) ? "" : String(held ?? "")}
      oninput={(e) => typeSecret(row, column.key, e.currentTarget.value)}
      {disabled} />
  {:else}
    <Input
      class={[INLINE, column.kind === "number" ? "tabular-nums" : "font-mono"].join(" ")}
      inputmode={column.kind === "number" ? "decimal" : undefined}
      aria-label={label}
      placeholder={defaultText(column)}
      value={String(held ?? "")}
      oninput={(e) => {
        const text = e.currentTarget.value;
        setCell(row, column.key, column.kind === "number" ? typedNumber(text) : text);
      }}
      {disabled} />
  {/if}
{/snippet}

{#snippet removeButton(row: Row, i: number)}
  <Button
    variant="ghost"
    size="icon-sm"
    class="row-menu"
    aria-label={`Remove ${nameOf(row, i)}`}
    onclick={() => remove(row)}>
    <Trash2Icon />
  </Button>
{/snippet}

{#snippet complaint(row: Row)}
  {#if trouble(row)}
    <p class="text-xs text-danger" role="alert">{trouble(row)}</p>
  {/if}
{/snippet}

<div
  bind:this={root}
  id={field.key}
  role="group"
  aria-label={field.label}
  aria-describedby={described}
  class="flex min-w-0 flex-col gap-2">
  {#each unplaced as message (message)}
    <p class="text-xs text-danger" role="alert">{message}</p>
  {/each}
  {#if !rows.length}
    <p class="text-sm text-fg-muted">None yet.</p>
  {:else if onALine}
    <!-- the whole width, rows parted by hairlines; on a phone each row stacks, its cells under
         their labels -->
    <table class="w-full">
      <thead class="max-md:sr-only">
        <tr class="border-b text-left text-xs font-medium text-fg-muted">
          {#if named}<th scope="col" class="px-2 py-1 font-medium">{nameLabel}</th>{/if}
          {#each columns as column (column.key)}
            <th
              scope="col"
              class="px-2 py-1 font-medium"
              title={helpParts(column.doc)
                .map((part) => part.text)
                .join("")}>{column.label}</th>
          {/each}
          {#if !disabled}<th scope="col" class="w-0 p-0"><span class="sr-only">Remove</span></th
            >{/if}
        </tr>
      </thead>
      <tbody>
        {#each rows as row, i (row.id)}
          <tr
            data-row={row.id}
            class="group relative border-b align-top max-md:flex max-md:flex-col max-md:gap-2 max-md:py-2">
            {#if named}
              <td class="max-md:flex max-md:flex-col max-md:gap-1">
                <span class="text-xs font-medium text-fg-muted md:hidden">{nameLabel}</span>
                {@render nameCell(row, i)}
              </td>
            {/if}
            {#each columns as column (column.key)}
              <td class="max-md:flex max-md:flex-col max-md:gap-1">
                <span class="text-xs font-medium text-fg-muted md:hidden">{column.label}</span>
                {@render cell(row, i, column)}
              </td>
            {/each}
            {#if !disabled}
              <td class="w-0 p-0 max-md:self-end">{@render removeButton(row, i)}</td>
            {/if}
          </tr>
          {#if trouble(row)}
            <tr><td colspan={columns.length + 2}>{@render complaint(row)}</td></tr>
          {/if}
        {/each}
      </tbody>
    </table>
  {:else}
    <ul class="flex flex-col divide-y border-y">
      {#each rows as row, i (row.id)}
        <li data-row={row.id}>
          <details class="group" open={added.has(row.id) || !!trouble(row)}>
            <summary
              class="flex cursor-pointer list-none items-center gap-1.5 px-2 py-2 text-sm font-medium hover:bg-hover [&::-webkit-details-marker]:hidden">
              <ChevronRightIcon
                class="size-4 shrink-0 text-fg-muted transition-transform duration-(--motion-fast) group-open:rotate-90" />
              <span class="min-w-0 font-mono wrap-anywhere">{row.name.trim() || "Unnamed"}</span>
            </summary>
            <div class="flex flex-col gap-3 px-2 pb-3">
              {@render complaint(row)}
              <div class="grid grid-cols-[minmax(0,1fr)] gap-3 md:grid-cols-2">
                <div class="flex flex-col gap-1">
                  <span class="text-xs font-medium text-fg-muted">{nameLabel}</span>
                  {@render nameCell(row, i)}
                </div>
                {#each columns as column (column.key)}
                  <div class="flex flex-col gap-1">
                    <span class="text-xs font-medium text-fg-muted">{column.label}</span>
                    {@render cell(row, i, column)}
                    <span class="text-xs text-pretty text-fg-subtle">
                      {#each helpParts(column.doc) as part, at (at)}{#if part.code}<code
                            class="font-mono wrap-anywhere">{part.text}</code
                          >{:else}{part.text}{/if}{/each}
                    </span>
                  </div>
                {/each}
              </div>
              {#if !disabled}
                <Button variant="ghost" size="sm" class="self-start" onclick={() => remove(row)}>
                  <Trash2Icon />Remove {nameOf(row, i)}
                </Button>
              {/if}
            </div>
          </details>
        </li>
      {/each}
    </ul>
  {/if}
  {#if !disabled}
    <Button
      data-add
      variant="outline"
      size="sm"
      class="self-start"
      aria-label={`Add to ${field.label}`}
      onclick={add}>
      <PlusIcon />Add
    </Button>
  {/if}
</div>
