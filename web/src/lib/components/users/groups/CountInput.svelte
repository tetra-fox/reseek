<script lang="ts">
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // a count a group can set: empty is not set, so a lower group's applies, and the button beside
  // the input is no limit, which the options hold as 0. A count that is `always` set has no empty:
  // there it is no limit too
  type Props = {
    id: string;
    /** the draft: empty, `0` for no limit, or the count as typed */
    value: string;
    /** what empty means, such as "Not set" */
    unset: string;
    /** what no limit is called, such as "Every slot" */
    none: string;
    always?: boolean;
    /**
     * why the count can't be emptied here, as for one a file sets; the field's hint says it, and
     * the button keeps its no limit
     */
    held?: string | null;
  };

  let { id, value = $bindable(), unset, none, always = false, held = null }: Props = $props();

  const unlimited = $derived(value.trim() === "0" || (always && !value.trim()));
  /** the button would empty the count, which `held` refuses */
  const stuck = $derived(unlimited && !always && !!held);
</script>

<div class="flex items-center gap-inline">
  <Input
    {id}
    class="min-w-0 tabular-nums"
    inputmode="numeric"
    placeholder={unlimited ? none : unset}
    value={unlimited ? "" : value}
    aria-describedby={held ? `${id}-hint` : undefined}
    oninput={(e) => (value = e.currentTarget.value || (always ? "0" : ""))} />
  <Tooltip text={(stuck && held) || undefined}>
    {#snippet child({ props })}
      <Button
        {...props}
        variant="outline"
        size="sm"
        class={["shrink-0", unlimited && "border-accent/40 bg-accent-soft text-accent-soft-fg"]}
        aria-pressed={unlimited}
        aria-disabled={stuck || undefined}
        onclick={() => (value = unlimited && !always ? "" : "0")}>{none}</Button>
    {/snippet}
  </Tooltip>
</div>
