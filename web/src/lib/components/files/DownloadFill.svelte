<script lang="ts">
  import { fillOf, type Transfer } from "#lib/transfers/states.js";
  import { motionFlash } from "#lib/ui/motion.js";

  type Props = {
    /** the file's download, when it has one */
    transfer?: Transfer;
    /** the file was just asked for and its download hasn't shown yet */
    starting?: boolean;
    /** a waiting download shows its empty track, where waiting is news */
    track?: boolean;
  };

  let { transfer, starting = false, track = true }: Props = $props();

  const fill = $derived(fillOf(transfer, starting));
  // a download that finishes while its row is shown flashes once, unless reduced motion is asked
  // for; a row that shows one already finished doesn't
  let flash = $state(false);
  // svelte-ignore state_referenced_locally
  let before = fill;
  $effect(() => {
    const now = fill;
    const finished = now === "done" && before !== null && before !== "done";
    before = now;
    const ms = motionFlash();
    if (!finished || !ms) return;
    flash = true;
    const timer = setTimeout(() => (flash = false), ms);
    return () => clearTimeout(timer);
  });
</script>

<!-- behind the row's cells: the row is an isolated stacking context, so this paints over its
     own tint and under its content. Not selectable, or text copied across rows while it shows
     would hold a blank line for it -->
{#if fill && (fill !== "done" || flash) && (fill !== "waiting" || track)}
  <span
    aria-hidden="true"
    class={[
      "pointer-events-none absolute inset-0 -z-10 select-none",
      (fill === "waiting" || fill === "moving") && "bg-accent/[0.06]",
      fill === "failed" && "bg-danger-soft/50",
      flash && "animate-done-flash"
    ]}
    data-fill={flash ? "flash" : fill}>
    {#if fill === "moving"}
      <span
        class="block h-full bg-accent/15 transition-[width] duration-(--motion-slow) ease-(--ease-out-soft)"
        style:width={`${transfer?.percentComplete ?? 0}%`}></span>
    {/if}
  </span>
{/if}
