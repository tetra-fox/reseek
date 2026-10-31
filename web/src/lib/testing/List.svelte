<script lang="ts">
  import { ListControl } from "#lib/lists/list.svelte.js";

  /** a row, or with `header` the header of the group named `group` */
  type Line = { key: string; group: string; header?: boolean };

  // a grouped list with a checkbox and a button per row, for the list controller's tests
  let {
    source,
    onopen = () => {},
    inspects,
    onfocus,
    headersFocus = false,
    named = false
  }: {
    source: { value: Line[] };
    onopen?: (line: Line) => void;
    /** the inspector can show a line */
    inspects?: (line: Line) => boolean;
    onfocus?: (line: Line) => void;
    /** headers take focus as rows do */
    headersFocus?: boolean;
    /** each line is named by its key, for type-ahead */
    named?: boolean;
  } = $props();

  // svelte-ignore state_referenced_locally
  const lines = source;
  const list = new ListControl<Line>({
    lines: () => lines.value,
    key: (l) => l.key,
    id: (l) => (l.header ? null : l.key),
    group: (l) => lines.value.filter((x) => !x.header && x.group === l.group).map((x) => x.key),
    onopen: (line) => onopen(line),
    // svelte-ignore state_referenced_locally
    inspects,
    onfocus: (line) => onfocus?.(line),
    // svelte-ignore state_referenced_locally
    headersFocus,
    // svelte-ignore state_referenced_locally
    typeahead: named ? (l) => l.key : undefined,
    page: () => 2
  });
</script>

<div
  role="grid"
  tabindex="0"
  aria-multiselectable="true"
  aria-activedescendant={list.activeId}
  {@attach list.attach}>
  {#each lines.value as line, i (line.key)}
    <div role="row" {...list.line(line, i)} class={{ focused: list.isFocused(line) }}>
      {#if !line.header}
        <button
          type="button"
          role="checkbox"
          aria-label={`Select ${line.key}`}
          aria-checked={list.selected.has(line.key)}
          onclick={(e) => list.tick(line.key, e.shiftKey)}></button>
        <button type="button">act</button>
      {/if}
      {line.key}
    </div>
  {/each}
</div>
